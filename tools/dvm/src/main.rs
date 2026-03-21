// delta VM - executes .dc bytecode files
// usage: dvm <file.dc> --entry <func_name> [--bench]
//    or: dvm <file.ds> --entry <func_name> [--bench]
//    or: dvm <file.ds> --entry <func_name> --compile [-o output] [--emit obj|exe]

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use delta_format::{
    encoding::{decode_func, Instr},
    file::{DataEntry, DcFile},
};

struct Args<'a> {
    path: &'a str,
    entry: &'a str,
    bench: bool,
    compile: bool,
    output: Option<&'a str>,
    emit: &'a str, // "obj" | "exe"
    no_console: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let parsed = parse_args(&args).unwrap_or_else(|| {
        eprintln!("usage: dvm <file> --entry <func> [--bench] [--compile [-o out] [--emit obj|exe] [--noconsole]]");
        std::process::exit(1);
    });

    if parsed.compile {
        let dc = load_file(parsed.path).unwrap_or_else(|e| {
            if !e.is_empty() { eprintln!("error: {e}"); }
            std::process::exit(1);
        });

        let emit_kind = match parsed.emit {
            "exe" => delta_cranelift::EmitKind::Exe,
            _     => delta_cranelift::EmitKind::Object,
        };

        let opts = delta_cranelift::CompileOptions {
            entry: parsed.entry.to_string(),
            opt_level: delta_cranelift::OptLevel::Default,
            emit: emit_kind,
            no_console: parsed.no_console,
        };

        let bytes = delta_cranelift::compile(&dc, &opts).unwrap_or_else(|e| {
            eprintln!("compile error: {e}");
            std::process::exit(1);
        });

        let ext = match parsed.emit {
            "exe" => {
                if cfg!(target_os = "windows") { ".exe" } else { "" }
            }
            _ => ".o",
        };
        let out = parsed.output.map(|s| s.to_string()).unwrap_or_else(|| {
            let base = parsed.path
                .trim_end_matches(".ds")
                .trim_end_matches(".dc");
            format!("{base}{ext}")
        });

        std::fs::write(&out, &bytes).unwrap_or_else(|e| {
            eprintln!("write error: {e}");
            std::process::exit(1);
        });
        // make exe executable on unix
        if parsed.emit == "exe" {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o755));
            }
        }
        eprintln!("compiled: {out} ({} bytes)", bytes.len());
        return;
    }

    let load_start = Instant::now();
    let dc = load_file(parsed.path).unwrap_or_else(|e| {
        if !e.is_empty() { eprintln!("error: {e}"); }
        std::process::exit(1);
    });
    let mut vm = Vm::new(dc).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });
    let load_time = load_start.elapsed();

    let run_start = Instant::now();
    let exit_code = vm.run(parsed.entry).unwrap_or_else(|e| {
        eprintln!("runtime error: {e}");
        std::process::exit(1);
    });
    let run_time = run_start.elapsed();

    if parsed.bench {
        eprintln!("--- bench ---");
        eprintln!("load+compile : {:>10.3} ms", load_time.as_secs_f64() * 1000.0);
        eprintln!("run          : {:>10.3} ms", run_time.as_secs_f64() * 1000.0);
        eprintln!("total        : {:>10.3} ms", (load_time + run_time).as_secs_f64() * 1000.0);
    }

    std::process::exit(exit_code);
}

fn parse_args(args: &[String]) -> Option<Args> {
    let path = args.get(1)?;
    let entry_flag = args.iter().position(|a| a == "--entry")?;
    let entry = args.get(entry_flag + 1)?;
    let bench = args.iter().any(|a| a == "--bench");
    let compile = args.iter().any(|a| a == "--compile");
    let no_console = args.iter().any(|a| a == "--noconsole" || a == "-noconsole" || a == "--no-console");
    let output = args.iter().position(|a| a == "-o")
        .and_then(|i| args.get(i + 1)).map(|s| s.as_str());
    let emit = args.iter().position(|a| a == "--emit")
        .and_then(|i| args.get(i + 1)).map(|s| s.as_str()).unwrap_or("obj");
    Some(Args { path: path.as_str(), entry: entry.as_str(), bench, compile, no_console, output, emit })
}

fn load_file(path: &str) -> Result<DcFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read '{path}': {e}"))?;
    if path.ends_with(".ds") {
        let src = String::from_utf8(bytes).map_err(|e| format!("invalid utf-8: {e}"))?;
        let prog = delta_asm::parse_and_analyze(&src)
            .ok_or_else(|| String::new())?; // diagnostics already printed; empty = silent exit
        delta_codegen::compile(&prog).map_err(|e| e.to_string())
    } else {
        DcFile::deserialize(&bytes).ok_or_else(|| format!("invalid .dc file: '{path}'"))
    }
}

// all registers are 64-bit slots
#[derive(Clone, Copy, Default)]
struct Reg(u64);

impl Reg {
    fn as_i64(self) -> i64   { i64::from_le_bytes(self.0.to_le_bytes()) }
    fn as_f64(self) -> f64   { f64::from_bits(self.0) }
    fn as_char(self) -> char { char::from_u32(self.0 as u32).unwrap_or('\0') }
    fn as_ptr(self) -> usize { self.0 as usize }
    fn from_i64(v: i64) -> Self   { Reg(u64::from_le_bytes(v.to_le_bytes())) }
    fn from_f64(v: f64) -> Self   { Reg(v.to_bits()) }
    fn from_char(c: char) -> Self { Reg(c as u64) }
    fn from_ptr(p: usize) -> Self { Reg(p as u64) }
}

// flat fixed-size instruction - no heap allocation, fits in two cache lines
// args are stored inline (max 8 args per call, covering all realistic cases)
#[derive(Clone, Copy)]
struct FlatInstr {
    op: u8,
    dst: u8,
    a: u8,
    b: u8,
    argc: u8,
    args: [u8; 8],
    // imm covers: immediate int/float/char/ptr values, jump targets, func indices
    imm: u64,
}

impl FlatInstr {
    fn from_instr(instr: &Instr) -> Self {
        match instr {
            Instr::A { op, dst, a, b } => Self {
                op: *op as u8, dst: *dst, a: *a, b: *b,
                argc: 0, args: [0; 8], imm: 0,
            },
            Instr::B { op, dst, imm } => Self {
                op: *op as u8, dst: *dst, a: 0, b: 0,
                argc: 0, args: [0; 8], imm: *imm as u64,
            },
            Instr::C { op, dst, func_idx, args } => {
                let mut flat_args = [0u8; 8];
                let argc = args.len().min(8);
                flat_args[..argc].copy_from_slice(&args[..argc]);
                Self {
                    op: *op as u8, dst: *dst, a: 0, b: 0,
                    argc: argc as u8, args: flat_args,
                    imm: *func_idx as u64,
                }
            }
            Instr::D { op, src } => Self {
                op: *op as u8, dst: *src, a: 0, b: 0,
                argc: 0, args: [0; 8], imm: 0,
            },
        }
    }
}

struct FuncSlot {
    instrs: Vec<FlatInstr>,
    reg_count: usize,
    name: String,
}

struct Vm {
    funcs: Vec<FuncSlot>,
    func_names: HashMap<String, usize>,
    data: Vec<DataEntry>,
    externs: Vec<String>,
    heap: HashMap<usize, Vec<u8>>,
    heap_next: usize,
    mono_start: Instant,
}

impl Vm {
    fn new(dc: DcFile) -> Result<Self, String> {
        let mut funcs = Vec::with_capacity(dc.funcs.len());
        let mut func_names = HashMap::new();

        for (i, f) in dc.funcs.iter().enumerate() {
            let decoded = decode_func(&dc.code, f.code_offset as usize, f.code_len as usize)
                .ok_or_else(|| format!("failed to decode function '{}'", f.name))?;
            let instrs: Vec<FlatInstr> = decoded.iter().map(FlatInstr::from_instr).collect();
            func_names.insert(f.name.clone(), i);
            funcs.push(FuncSlot {
                instrs,
                reg_count: f.reg_count as usize,
                name: f.name.clone(),
            });
        }

        let externs = dc.externs.iter().map(|e| e.name.clone()).collect();

        Ok(Self {
            funcs,
            func_names,
            data: dc.data,
            externs,
            heap: HashMap::new(),
            heap_next: 1,
            mono_start: Instant::now(),
        })
    }

    fn run(&mut self, entry: &str) -> Result<i32, String> {
        let idx = *self.func_names.get(entry)
            .ok_or_else(|| format!("function '{entry}' not found"))?;
        Ok(self.call_func(idx, &[])?.as_i64() as i32)
    }

    fn call_func(&mut self, func_idx: usize, args: &[Reg]) -> Result<Reg, String> {
        let (reg_count, instr_count) = {
            let f = &self.funcs[func_idx];
            (f.reg_count.max(args.len()), f.instrs.len())
        };

        let mut regs = vec![Reg::default(); reg_count];
        for (i, a) in args.iter().enumerate() {
            regs[i] = *a;
        }

        let mut pc = 0usize;

        loop {
            // safety: pc is only set to valid indices (jump translation guarantees it)
            // and we check against instr_count below for the fallthrough case
            let FlatInstr { op, dst, a, b, argc, args: iargs, imm } =
                self.funcs[func_idx].instrs[pc];
            pc += 1;

            match op {
                // --- arithmetic int ---
                0x01 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64().wrapping_add(regs[b as usize].as_i64())),
                0x02 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64().wrapping_sub(regs[b as usize].as_i64())),
                0x03 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64().wrapping_mul(regs[b as usize].as_i64())),
                0x04 => {
                    let rb = regs[b as usize].as_i64();
                    if rb == 0 { return Err("division by zero".into()); }
                    regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64() / rb);
                }
                // --- arithmetic float ---
                0x05 => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64() + regs[b as usize].as_f64()),
                0x06 => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64() - regs[b as usize].as_f64()),
                0x07 => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64() * regs[b as usize].as_f64()),
                0x08 => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64() / regs[b as usize].as_f64()),
                // --- comparisons int ---
                0x10 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_i64() == regs[b as usize].as_i64()) as i64),
                0x11 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_i64() != regs[b as usize].as_i64()) as i64),
                0x12 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_i64() <  regs[b as usize].as_i64()) as i64),
                0x13 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_i64() <= regs[b as usize].as_i64()) as i64),
                0x14 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_i64() >  regs[b as usize].as_i64()) as i64),
                0x15 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_i64() >= regs[b as usize].as_i64()) as i64),
                // --- comparisons float ---
                0x16 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_f64() == regs[b as usize].as_f64()) as i64),
                0x17 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_f64() != regs[b as usize].as_f64()) as i64),
                0x18 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_f64() <  regs[b as usize].as_f64()) as i64),
                0x19 => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_f64() <= regs[b as usize].as_f64()) as i64),
                0x1A => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_f64() >  regs[b as usize].as_f64()) as i64),
                0x1B => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_f64() >= regs[b as usize].as_f64()) as i64),
                // --- comparisons char ---
                0x1C => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_char() == regs[b as usize].as_char()) as i64),
                0x1D => regs[dst as usize] = Reg::from_i64((regs[a as usize].as_char() != regs[b as usize].as_char()) as i64),
                // --- loads ---
                0x20 => regs[dst as usize] = Reg::from_i64(imm as i32 as i64),
                0x21 => regs[dst as usize] = Reg::from_f64(f32::from_bits(imm as u32) as f64),
                0x22 => regs[dst as usize] = Reg::from_char(char::from_u32(imm as u32).unwrap_or('\0')),
                0x23 => regs[dst as usize] = Reg::from_ptr(imm as usize | DATA_PTR_TAG),
                // --- memory ---
                0x30 => { let p = self.heap_alloc(imm as usize); regs[dst as usize] = Reg::from_ptr(p); }
                0x31 => { let s = regs[a as usize].as_i64() as usize; regs[dst as usize] = Reg::from_ptr(self.heap_alloc(s)); }
                0x32 => { self.heap.remove(&regs[dst as usize].as_ptr()); }
                0x33 => { self.heap_store(regs[a as usize].as_ptr(), regs[b as usize])?; }
                0x34 => { regs[dst as usize] = self.heap_read(regs[b as usize].as_ptr())?; }
                // --- jumps ---
                0x40 => { pc = imm as usize; }
                0x41 => { if regs[dst as usize].as_i64() != 0 { pc = imm as usize; } }
                0x42 => { if regs[dst as usize].as_i64() == 0 { pc = imm as usize; } }
                // --- calls ---
                0x50 => {
                    let call_args: Vec<Reg> = iargs[..argc as usize].iter().map(|&i| regs[i as usize]).collect();
                    regs[dst as usize] = self.call_func(imm as usize, &call_args)?;
                }
                0x51 => {
                    let call_args: Vec<Reg> = iargs[..argc as usize].iter().map(|&i| regs[i as usize]).collect();
                    self.call_func(imm as usize, &call_args)?;
                }
                0x52 => {
                    let call_args: Vec<Reg> = iargs[..argc as usize].iter().map(|&i| regs[i as usize]).collect();
                    regs[dst as usize] = self.call_extern(imm as usize, &call_args)?;
                }
                0x53 => {
                    let call_args: Vec<Reg> = iargs[..argc as usize].iter().map(|&i| regs[i as usize]).collect();
                    self.call_extern(imm as usize, &call_args)?;
                }
                // CallPtr (0xE1): imm = register index holding the function pointer
                0xE1 => {
                    let fptr = regs[imm as usize].as_ptr();
                    let call_args: Vec<Reg> = iargs[..argc as usize].iter().map(|&i| regs[i as usize]).collect();
                    let fidx = fptr & !FUNC_PTR_TAG;
                    regs[dst as usize] = self.call_func(fidx, &call_args)?;
                }
                // CallPtrVoid (0xE2)
                0xE2 => {
                    let fptr = regs[imm as usize].as_ptr();
                    let call_args: Vec<Reg> = iargs[..argc as usize].iter().map(|&i| regs[i as usize]).collect();
                    let fidx = fptr & !FUNC_PTR_TAG;
                    self.call_func(fidx, &call_args)?;
                }
                // --- return ---
                0x60 => return Ok(regs[dst as usize]),
                0x61 => return Ok(Reg::default()),
                // --- print ---
                0x70 => print!("{}", regs[dst as usize].as_i64()),
                0x71 => print!("{}", regs[dst as usize].as_f64()),
                0x72 => print!("{}", regs[dst as usize].as_char()),
                0x73 => { let s = self.read_str_ptr(regs[dst as usize])?; print!("{s}"); }
                // --- time ---
                0x80 => {
                    let ns = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos() as i64;
                    regs[dst as usize] = Reg::from_i64(ns);
                }
                0x81 => {
                    let ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64;
                    regs[dst as usize] = Reg::from_i64(ms);
                }
                0x82 => {
                    let ns = self.mono_start.elapsed().as_nanos() as i64;
                    regs[dst as usize] = Reg::from_i64(ns);
                }
                // --- extended arithmetic binary ---
                0x09 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64().wrapping_rem(regs[b as usize].as_i64())),
                0x0A => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64() % regs[b as usize].as_f64()),
                0x0B => {
                    let base = regs[a as usize].as_i64();
                    let exp = regs[b as usize].as_i64();
                    regs[dst as usize] = Reg::from_i64(if exp < 0 { 0 } else { base.wrapping_pow(exp as u32) });
                }
                0x0C => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64().powf(regs[b as usize].as_f64())),
                // --- extended arithmetic unary ---
                0x0D => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64().wrapping_neg()),
                0x0E => regs[dst as usize] = Reg::from_f64(-regs[a as usize].as_f64()),
                0x0F => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64().abs()),
                0x8F => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64().abs()),
                0x8E => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_f64().sqrt()),
                // --- type casts ---
                0x90 => regs[dst as usize] = Reg::from_f64(regs[a as usize].as_i64() as f64),
                0x91 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_f64() as i64),
                0x92 => regs[dst as usize] = Reg::from_char(char::from_u32(regs[a as usize].as_i64() as u32).unwrap_or('\0')),
                0x93 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_char() as i64),
                0x94 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_ptr() as i64),
                // --- string / char ops ---
                0xA0 => {
                    let len = self.str_len(regs[a as usize])?;
                    regs[dst as usize] = Reg::from_i64(len as i64);
                }
                0xA1 => {
                    let eq = self.str_eq(regs[a as usize], regs[b as usize])?;
                    regs[dst as usize] = Reg::from_i64(eq as i64);
                }
                0xA2 => {
                    let ch = self.str_char_at(regs[a as usize], regs[b as usize].as_i64() as usize)?;
                    regs[dst as usize] = Reg::from_char(ch);
                }
                0xA3 => {
                    let c = regs[a as usize].as_char().to_uppercase().next().unwrap_or('\0');
                    regs[dst as usize] = Reg::from_char(c);
                }
                0xA4 => {
                    let c = regs[a as usize].as_char().to_lowercase().next().unwrap_or('\0');
                    regs[dst as usize] = Reg::from_char(c);
                }
                0xA5 => {
                    let s = regs[a as usize].as_i64().to_string();
                    let ptr = self.heap_alloc_str(&s);
                    regs[dst as usize] = Reg::from_ptr(ptr);
                }
                0xA6 => {
                    let s = format!("{}", regs[a as usize].as_f64());
                    let ptr = self.heap_alloc_str(&s);
                    regs[dst as usize] = Reg::from_ptr(ptr);
                }

                // --- arrays ---
                // ArrNew (format B): dst = new array of imm elements
                0xC0 => {
                    let len = imm as usize;
                    let ptr = self.arr_alloc(len);
                    regs[dst as usize] = Reg::from_ptr(ptr);
                }
                // ArrNewReg (format A): dst = new array of a elements
                0xC1 => {
                    let len = regs[a as usize].as_i64() as usize;
                    let ptr = self.arr_alloc(len);
                    regs[dst as usize] = Reg::from_ptr(ptr);
                }
                // ArrGet (format A): dst = arr[b]
                0xC2 => {
                    let val = self.arr_get(regs[a as usize].as_ptr(), regs[b as usize].as_i64() as usize)?;
                    regs[dst as usize] = val;
                }
                // ArrSet (format A): arr[dst][a] = b  (dst = arr ptr)
                0xC3 => {
                    self.arr_set(regs[dst as usize].as_ptr(), regs[a as usize].as_i64() as usize, regs[b as usize])?;
                }
                // ArrLen (format A): dst = len(a)
                0xC4 => {
                    let len = self.arr_len(regs[a as usize].as_ptr())?;
                    regs[dst as usize] = Reg::from_i64(len as i64);
                }
                // ArrFree (format D): free array at src
                0xC5 => {
                    self.arr_free(regs[dst as usize].as_ptr());
                }

                // --- stdin input ---
                0xB0 => { // ReadChar
                    use std::io::Read;
                    let mut buf = [0u8; 4];
                    let c = if std::io::stdin().read(&mut buf[..1]).is_ok() && buf[0] != 0 {
                        char::from(buf[0])
                    } else { '\0' };
                    regs[dst as usize] = Reg::from_char(c);
                }
                0xB1 => { // ReadInt
                    let mut s = String::new();
                    std::io::stdin().read_line(&mut s).ok();
                    let n = s.trim().parse::<i64>().unwrap_or(0);
                    regs[dst as usize] = Reg::from_i64(n);
                }
                0xB2 => { // ReadFloat
                    let mut s = String::new();
                    std::io::stdin().read_line(&mut s).ok();
                    let f = s.trim().parse::<f64>().unwrap_or(0.0);
                    regs[dst as usize] = Reg::from_f64(f);
                }
                0xB3 => { // ReadLine
                    let mut s = String::new();
                    std::io::stdin().read_line(&mut s).ok();
                    let trimmed = s.trim_end_matches('\n').trim_end_matches('\r').to_string();
                    let ptr = self.heap_alloc_str(&trimmed);
                    regs[dst as usize] = Reg::from_ptr(ptr);
                }

                // --- bitwise ---
                0xD0 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64() & regs[b as usize].as_i64()),
                0xD1 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64() | regs[b as usize].as_i64()),
                0xD2 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64() ^ regs[b as usize].as_i64()),
                0xD3 => regs[dst as usize] = Reg::from_i64(!regs[a as usize].as_i64()),
                0xD4 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64() << (regs[b as usize].as_i64() & 63)),
                0xD5 => regs[dst as usize] = Reg::from_i64(regs[a as usize].as_i64() >> (regs[b as usize].as_i64() & 63)),

                // FuncPtr (format B): dst = address-of func[imm]
                // We encode func index as a tagged pointer: high bit set + func_idx
                0xE0 => {
                    regs[dst as usize] = Reg::from_ptr(imm as usize | FUNC_PTR_TAG);
                }

                // Panic (format D): print message at src, exit 1
                0xE3 => {
                    let msg = self.read_str_ptr(regs[dst as usize])
                        .unwrap_or_else(|_| "<invalid panic message>".into());
                    eprintln!("panic: {msg}");
                    std::process::exit(1);
                }

                x => return Err(format!("unknown opcode {x:#04x} at pc {}", pc - 1)),
            }

            if pc > instr_count {
                return Err(format!("execution fell off end of '{}'", self.funcs[func_idx].name));
            }
        }
    }

    fn call_extern(&mut self, idx: usize, args: &[Reg]) -> Result<Reg, String> {
        let name = self.externs.get(idx)
            .ok_or_else(|| format!("invalid extern index {idx}"))?.clone();
        match name.as_str() {
            "putchar" => {
                let v = args.first().map_or(0i64, |r| r.as_i64()) as u8 as char;
                print!("{v}");
                Ok(Reg::from_i64(v as i64))
            }
            "putstr" => {
                let s = self.read_str_ptr(args.first().copied().unwrap_or_default())?;
                print!("{s}");
                Ok(Reg::default())
            }
            "printf" => {
                let fmt = self.read_str_ptr(args.first().copied().unwrap_or_default())?;
                let mut arg_idx = 1usize;
                let mut out = String::new();
                let mut chars = fmt.chars().peekable();
                while let Some(c) = chars.next() {
                    if c == '%' {
                        match chars.next() {
                            Some('d') | Some('i') | Some('l') => {
                                // skip 'l' modifier if present
                                let spec = if fmt.contains("%ld") || fmt.contains("%li") { chars.next(); 'd' } else { 'd' };
                                let _ = spec;
                                let v = args.get(arg_idx).map_or(0, |r| r.as_i64());
                                out.push_str(&v.to_string());
                                arg_idx += 1;
                            }
                            Some('s') => {
                                let v = args.get(arg_idx).copied().unwrap_or_default();
                                let s = self.read_str_ptr(v).unwrap_or_default();
                                out.push_str(&s);
                                arg_idx += 1;
                            }
                            Some('f') | Some('g') | Some('e') => {
                                let v = args.get(arg_idx).map_or(0.0, |r| r.as_f64());
                                out.push_str(&format!("{v}"));
                                arg_idx += 1;
                            }
                            Some('c') => {
                                let v = args.get(arg_idx).map_or(0i64, |r| r.as_i64()) as u8 as char;
                                out.push(v);
                                arg_idx += 1;
                            }
                            Some('%') => out.push('%'),
                            Some(x)   => { out.push('%'); out.push(x); }
                            None      => out.push('%'),
                        }
                    } else {
                        out.push(c);
                    }
                }
                print!("{out}");
                Ok(Reg::from_i64(out.len() as i64))
            }
            "malloc" => {
                let size = args.first().map_or(0, |r| r.as_i64()) as usize;
                let ptr = self.heap_alloc(size);
                Ok(Reg::from_ptr(ptr))
            }
            "free" => {
                let ptr = args.first().map_or(0, |r| r.as_ptr());
                self.heap.remove(&ptr);
                Ok(Reg::default())
            }
            "exit" => {
                let code = args.first().map_or(0, |r| r.as_i64()) as i32;
                std::process::exit(code);
            }
            "strlen" => {
                let s = self.read_str_ptr(args.first().copied().unwrap_or_default())?;
                Ok(Reg::from_i64(s.len() as i64))
            }
            "strcmp" => {
                let a = self.read_str_ptr(args.first().copied().unwrap_or_default())?;
                let b = self.read_str_ptr(args.get(1).copied().unwrap_or_default())?;
                Ok(Reg::from_i64(a.cmp(&b) as i64))
            }
            other => Err(format!("unknown extern '{other}' - add to .extern and use cranelift backend, or call via call.ext")),
        }
    }

    fn read_str_ptr(&self, ptr: Reg) -> Result<String, String> {
        let bytes = self.str_bytes(ptr)?;
        String::from_utf8(bytes).map_err(|_| "invalid utf-8 in string".into())
    }

    fn heap_alloc(&mut self, size: usize) -> usize {
        let ptr = self.heap_next;
        self.heap_next += 1;
        self.heap.insert(ptr, vec![0u8; size]);
        ptr
    }

    fn heap_alloc_str(&mut self, s: &str) -> usize {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0); // null-terminate
        let ptr = self.heap_next;
        self.heap_next += 1;
        self.heap.insert(ptr, bytes);
        ptr
    }

    fn str_bytes(&self, ptr: Reg) -> Result<Vec<u8>, String> {
        let raw = ptr.as_ptr();
        if raw & DATA_PTR_TAG != 0 {
            let idx = raw & !DATA_PTR_TAG;
            match self.data.get(idx) {
                Some(DataEntry::Str(bytes)) => Ok(bytes.iter().copied().take_while(|&b| b != 0).collect()),
                _ => Err(format!("data index {idx} is not a string")),
            }
        } else {
            let bytes = self.heap.get(&raw)
                .ok_or_else(|| format!("invalid heap pointer {raw:#x}"))?;
            Ok(bytes.iter().copied().take_while(|&b| b != 0).collect())
        }
    }

    fn str_len(&self, ptr: Reg) -> Result<usize, String> {
        Ok(self.str_bytes(ptr)?.len())
    }

    fn str_eq(&self, a: Reg, b: Reg) -> Result<bool, String> {
        Ok(self.str_bytes(a)? == self.str_bytes(b)?)
    }

    fn str_char_at(&self, ptr: Reg, idx: usize) -> Result<char, String> {
        let bytes = self.str_bytes(ptr)?;
        let s = String::from_utf8(bytes).map_err(|_| "invalid utf-8".to_string())?;
        s.chars().nth(idx).ok_or_else(|| format!("string index {idx} out of bounds"))
    }

    fn heap_store(&mut self, ptr: usize, val: Reg) -> Result<(), String> {
        let slot = self.heap.get_mut(&ptr)
            .ok_or_else(|| format!("store to invalid pointer {ptr:#x}"))?;
        let bytes = val.0.to_le_bytes();
        let len = slot.len().min(8);
        slot[..len].copy_from_slice(&bytes[..len]);
        Ok(())
    }

    fn heap_read(&self, ptr: usize) -> Result<Reg, String> {
        let slot = self.heap.get(&ptr)
            .ok_or_else(|| format!("read from invalid pointer {ptr:#x}"))?;
        let mut bytes = [0u8; 8];
        let len = slot.len().min(8);
        bytes[..len].copy_from_slice(&slot[..len]);
        Ok(Reg(u64::from_le_bytes(bytes)))
    }

    // arrays: heap layout = [len:i64 le][elem0:i64 le]...[elemN-1:i64 le]
    fn arr_alloc(&mut self, len: usize) -> usize {
        let bytes = (len + 1) * 8;
        let ptr = self.heap_next;
        self.heap_next += 1;
        let mut buf = vec![0u8; bytes];
        // write length at offset 0
        buf[..8].copy_from_slice(&(len as i64).to_le_bytes());
        self.heap.insert(ptr, buf);
        ptr
    }

    fn arr_len(&self, ptr: usize) -> Result<usize, String> {
        let buf = self.heap.get(&ptr)
            .ok_or_else(|| format!("arr_len: invalid pointer {ptr:#x}"))?;
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[..8]);
        Ok(i64::from_le_bytes(b) as usize)
    }

    fn arr_get(&self, ptr: usize, idx: usize) -> Result<Reg, String> {
        let buf = self.heap.get(&ptr)
            .ok_or_else(|| format!("arr_get: invalid pointer {ptr:#x}"))?;
        let len = i64::from_le_bytes(buf[..8].try_into().unwrap()) as usize;
        if idx >= len { return Err(format!("arr_get: index {idx} out of bounds (len={len})")); }
        let off = (idx + 1) * 8;
        let mut b = [0u8; 8];
        b.copy_from_slice(&buf[off..off + 8]);
        Ok(Reg(u64::from_le_bytes(b)))
    }

    fn arr_set(&mut self, ptr: usize, idx: usize, val: Reg) -> Result<(), String> {
        let buf = self.heap.get_mut(&ptr)
            .ok_or_else(|| format!("arr_set: invalid pointer {ptr:#x}"))?;
        let len = i64::from_le_bytes(buf[..8].try_into().unwrap()) as usize;
        if idx >= len { return Err(format!("arr_set: index {idx} out of bounds (len={len})")); }
        let off = (idx + 1) * 8;
        buf[off..off + 8].copy_from_slice(&val.0.to_le_bytes());
        Ok(())
    }

    fn arr_free(&mut self, ptr: usize) {
        self.heap.remove(&ptr);
    }
}

// top bit of usize - tags data-section pointers so they don't collide with heap pointers
const DATA_PTR_TAG: usize = 1 << (usize::BITS - 1);
// second-top bit tags function pointers (func index stored in low bits)
const FUNC_PTR_TAG: usize = 1 << (usize::BITS - 2);
