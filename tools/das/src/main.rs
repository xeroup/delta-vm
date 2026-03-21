// das - delta assembler disassembler
//
// usage:
//   das <file.dc>               disassemble bytecode
//   das <file.dc> --info        show file summary only
//   das <file.ds>               parse + compile + disassemble (round-trip check)
//   das <file.dc> -f <func>     disassemble one function only
//   das <file.dc> --hex         show raw hex alongside disassembly

use delta_format::{
    encoding::{decode_func, Instr},
    file::{DataEntry, DcFile},
    opcode::Op,
};

struct Args {
    path: String,
    func_filter: Option<String>,
    info_only: bool,
    show_hex: bool,
}

fn main() {
    let raw: Vec<String> = std::env::args().collect();
    let args = match parse_args(&raw) {
        Some(a) => a,
        None => {
            eprintln!("usage: das <file.dc|.ds> [--info] [-f <func>] [--hex]");
            std::process::exit(1);
        }
    };

    let dc = load_file(&args.path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        std::process::exit(1);
    });

    if args.info_only {
        print_info(&dc);
        return;
    }

    print_disasm(&dc, &args);
}

fn parse_args(raw: &[String]) -> Option<Args> {
    let path = raw.get(1)?.clone();
    let func_filter = raw.iter().position(|a| a == "-f")
        .and_then(|i| raw.get(i + 1)).cloned();
    let info_only = raw.iter().any(|a| a == "--info");
    let show_hex  = raw.iter().any(|a| a == "--hex");
    Some(Args { path, func_filter, info_only, show_hex })
}

fn load_file(path: &str) -> Result<DcFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("cannot read '{path}': {e}"))?;
    if path.ends_with(".ds") {
        let src = String::from_utf8(bytes).map_err(|e| format!("invalid utf-8: {e}"))?;
        let prog = delta_asm::parse_and_analyze(&src).ok_or("assembly failed")?;
        delta_codegen::compile(&prog).map_err(|e| e.to_string())
    } else {
        DcFile::deserialize(&bytes).ok_or_else(|| format!("invalid .dc file: '{path}'"))
    }
}

// -- info ----------------------------------------------------------------------

fn print_info(dc: &DcFile) {
    let total_code: usize = dc.funcs.iter().map(|f| f.code_len as usize).sum();
    println!("=== delta file info ===");
    println!("  functions : {}", dc.funcs.len());
    println!("  code      : {} bytes", total_code);
    println!("  data      : {} entries", dc.data.len());
    println!("  externs   : {}", dc.externs.len());
    println!();

    if !dc.funcs.is_empty() {
        println!("functions:");
        for (i, f) in dc.funcs.iter().enumerate() {
            println!("  [{i:>3}] {:<24} regs={:<3} params={} offset={:#06x} len={}",
                f.name, f.reg_count, f.param_count, f.code_offset, f.code_len);
        }
        println!();
    }

    if !dc.externs.is_empty() {
        println!("externs:");
        for (i, e) in dc.externs.iter().enumerate() {
            println!("  [{i:>3}] {} (params={})", e.name, e.param_count);
        }
        println!();
    }

    if !dc.data.is_empty() {
        println!("data:");
        for (i, d) in dc.data.iter().enumerate() {
            match d {
                DataEntry::Str(b) => {
                    let s = String::from_utf8_lossy(&b[..b.len().saturating_sub(1)]);
                    println!("  [{i:>3}] str  \"{}\"", escape_str(&s));
                }
                DataEntry::Int(n)   => println!("  [{i:>3}] int  {n}"),
                DataEntry::Float(f) => println!("  [{i:>3}] float {f}"),
            }
        }
        println!();
    }
}

// -- disassembly ---------------------------------------------------------------

fn print_disasm(dc: &DcFile, args: &Args) {
    println!("; delta bytecode - {} function(s)", dc.funcs.len());
    if !dc.externs.is_empty() {
        println!(";");
        println!("; externs:");
        for (i, e) in dc.externs.iter().enumerate() {
            println!(";   [{i}] {}", e.name);
        }
    }
    if !dc.data.is_empty() {
        println!(";");
        println!("; data:");
        for (i, d) in dc.data.iter().enumerate() {
            match d {
                DataEntry::Str(b) => {
                    let s = String::from_utf8_lossy(&b[..b.len().saturating_sub(1)]);
                    println!(";   [{i}] str  \"{}\"", escape_str(&s));
                }
                DataEntry::Int(n)   => println!(";   [{i}] int  {n}"),
                DataEntry::Float(f) => println!(";   [{i}] float {f}"),
            }
        }
    }
    println!();

    for func in dc.funcs.iter() {
        if let Some(ref filter) = args.func_filter {
            if &func.name != filter { continue; }
        }

        println!("fn {} (regs={}, params={}):", func.name, func.reg_count, func.param_count);

        let raw_bytes = dc.code
            .get(func.code_offset as usize .. func.code_offset as usize + func.code_len as usize)
            .unwrap_or(&[]);

        let instrs = decode_func(&dc.code, func.code_offset as usize, func.code_len as usize)
            .unwrap_or_default();

        // build byte-offset list for jump label resolution
        let mut offsets: Vec<usize> = Vec::with_capacity(instrs.len());
        let mut pos = 0usize;
        for instr in &instrs {
            offsets.push(pos);
            pos += instr_byte_len(instr);
        }

        let mut byte_pos = 0usize;
        for (ii, instr) in instrs.iter().enumerate() {
            let blen = instr_byte_len(instr);
            let slice = raw_bytes.get(byte_pos..byte_pos + blen).unwrap_or(&[]);

            // check if any jump targets this instruction index -> print label
            let is_target = instrs.iter().enumerate().any(|(j, instr_j)| {
                j != ii && is_jump_to(instr_j, j, ii)
            });
            if is_target {
                println!("  .L{ii}:");
            }

            let hex_col = if args.show_hex {
                let h: Vec<String> = slice.iter().map(|b| format!("{b:02x}")).collect();
                format!("{:<20} ", h.join(" "))
            } else {
                String::new()
            };

            let text = format_instr(instr, &offsets, dc);
            println!("  {:04x}:  {hex_col}{text}", byte_pos);
            byte_pos += blen;
        }

        println!();
    }
}

fn is_jump_to(instr: &Instr, instr_idx: usize, target_idx: usize) -> bool {
    let _ = instr_idx;
    match instr {
        Instr::B { op: Op::Jmp, imm, .. }
        | Instr::B { op: Op::JmpIf, imm, .. }
        | Instr::B { op: Op::JmpIfNot, imm, .. } => *imm as usize == target_idx,
        _ => false,
    }
}

// -- instruction formatter -----------------------------------------------------

fn format_instr(instr: &Instr, offsets: &[usize], dc: &DcFile) -> String {
    match instr {
        Instr::A { op, dst, a, b } => {
            let m = op_mnemonic(*op);
            match op {
                Op::Store     => format!("{m:<14} r{a}, r{b}"),
                Op::ArrSet    => format!("{m:<14} r{dst}[r{a}], r{b}"),
                Op::Read      => format!("{m:<14} r{dst}, [r{b}]"),
                Op::ArrGet    => format!("{m:<14} r{dst}, r{a}[r{b}]"),
                // unary (b unused)
                Op::NegInt | Op::NegFloat | Op::AbsInt | Op::AbsFloat | Op::SqrtFloat |
                Op::IntToFloat | Op::FloatToInt | Op::IntToChar | Op::CharToInt | Op::PtrToInt |
                Op::CharToUpper | Op::CharToLower | Op::IntToStr | Op::FloatToStr |
                Op::StrLen | Op::ArrLen | Op::ArrNewReg | Op::AllocReg =>
                    format!("{m:<14} r{dst}, r{a}"),
                _ => format!("{m:<14} r{dst}, r{a}, r{b}"),
            }
        }

        Instr::B { op, dst, imm } => {
            let m = op_mnemonic(*op);
            match op {
                Op::LoadInt   => { let v = *imm as i32; format!("{m:<14} r{dst}, {v}") }
                Op::LoadFloat => { let v = f32::from_bits(*imm); format!("{m:<14} r{dst}, {v}") }
                Op::LoadChar  => {
                    let c = char::from_u32(*imm).unwrap_or('?');
                    format!("{m:<14} r{dst}, '{}'", escape_char(c))
                }
                Op::LoadPtr   => {
                    let cmt = data_comment(dc, *imm as usize);
                    format!("{m:<14} r{dst}, data[{imm}]{cmt}")
                }
                Op::FuncPtr => {
                    let name = dc.funcs.get(*imm as usize).map(|f| f.name.as_str()).unwrap_or("?");
                    format!("{m:<14} r{dst}, {name}")
                }
                Op::Jmp       => format!("{m:<14} {}", instr_idx_to_label(*imm as usize, offsets)),
                Op::JmpIf | Op::JmpIfNot => {
                    format!("{m:<14} r{dst}, {}", instr_idx_to_label(*imm as usize, offsets))
                }
                _ => format!("{m:<14} r{dst}, {imm}"),
            }
        }

        Instr::C { op, dst, func_idx, args } => {
            let m = op_mnemonic(*op);
            let arg_list: Vec<String> = args.iter().map(|r| format!("r{r}")).collect();
            let args_str = arg_list.join(", ");
            match op {
                Op::CallPtr => {
                    format!("{m:<14} r{dst}, r{func_idx}({args_str})")
                }
                Op::CallPtrVoid => {
                    format!("{m:<14} r{func_idx}({args_str})")
                }
                Op::Call | Op::CallVoid => {
                    let target = dc.funcs.get(*func_idx as usize)
                        .map(|f| f.name.clone()).unwrap_or_else(|| format!("fn[{func_idx}]"));
                    match op {
                        Op::CallVoid => format!("{m:<14} {target}({args_str})"),
                        _ => format!("{m:<14} r{dst}, {target}({args_str})"),
                    }
                }
                _ => {
                    let target = dc.externs.get(*func_idx as usize)
                        .map(|e| e.name.clone()).unwrap_or_else(|| format!("ext[{func_idx}]"));
                    match op {
                        Op::CallExtVoid => format!("{m:<14} {target}({args_str})"),
                        _ => format!("{m:<14} r{dst}, {target}({args_str})"),
                    }
                }
            }
        }

        Instr::D { op, src } => {
            let m = op_mnemonic(*op);
            match op {
                Op::RetVoid => m.to_string(),
                _ => format!("{m:<14} r{src}"),
            }
        }
    }
}

// -- helpers -------------------------------------------------------------------

fn op_mnemonic(op: Op) -> &'static str {
    match op {
        Op::AddInt      => "add.i",
        Op::SubInt      => "sub.i",
        Op::MulInt      => "mul.i",
        Op::DivInt      => "div.i",
        Op::ModInt      => "mod.i",
        Op::PowInt      => "pow.i",
        Op::NegInt      => "neg.i",
        Op::AbsInt      => "abs.i",
        Op::AddFloat    => "add.f",
        Op::SubFloat    => "sub.f",
        Op::MulFloat    => "mul.f",
        Op::DivFloat    => "div.f",
        Op::ModFloat    => "mod.f",
        Op::PowFloat    => "pow.f",
        Op::NegFloat    => "neg.f",
        Op::AbsFloat    => "abs.f",
        Op::SqrtFloat   => "sqrt.f",
        Op::EqInt       => "eq.i",
        Op::NeInt       => "ne.i",
        Op::LtInt       => "lt.i",
        Op::LeInt       => "le.i",
        Op::GtInt       => "gt.i",
        Op::GeInt       => "ge.i",
        Op::EqFloat     => "eq.f",
        Op::NeFloat     => "ne.f",
        Op::LtFloat     => "lt.f",
        Op::LeFloat     => "le.f",
        Op::GtFloat     => "gt.f",
        Op::GeFloat     => "ge.f",
        Op::EqChar      => "eq.c",
        Op::NeChar      => "ne.c",
        Op::LoadInt     => "load.i",
        Op::LoadFloat   => "load.f",
        Op::LoadChar    => "load.c",
        Op::LoadPtr     => "load.p",
        Op::Alloc       => "alloc",
        Op::AllocReg    => "alloc.r",
        Op::Free        => "free",
        Op::Store       => "store",
        Op::Read        => "read",
        Op::Jmp         => "jmp",
        Op::JmpIf       => "jmp.if",
        Op::JmpIfNot    => "jmp.ifn",
        Op::Call        => "call",
        Op::CallVoid    => "call.v",
        Op::CallExt     => "call.e",
        Op::CallExtVoid => "call.ev",
        Op::Ret         => "ret",
        Op::RetVoid     => "ret.v",
        Op::PrintInt    => "print.i",
        Op::PrintFloat  => "print.f",
        Op::PrintChar   => "print.c",
        Op::PrintPtr    => "print.p",
        Op::TimeNs      => "time.ns",
        Op::TimeMs      => "time.ms",
        Op::TimeMonoNs  => "time.mono",
        Op::IntToFloat  => "cast.if",
        Op::FloatToInt  => "cast.fi",
        Op::IntToChar   => "cast.ic",
        Op::CharToInt   => "cast.ci",
        Op::PtrToInt    => "cast.pi",
        Op::StrLen      => "str.len",
        Op::StrEq       => "str.eq",
        Op::StrCharAt   => "str.at",
        Op::CharToUpper => "char.up",
        Op::CharToLower => "char.lo",
        Op::IntToStr    => "int.str",
        Op::FloatToStr  => "flt.str",
        Op::ReadChar    => "read.c",
        Op::ReadInt     => "read.i",
        Op::ReadFloat   => "read.f",
        Op::ReadLine    => "read.l",
        Op::ArrNew      => "arr.new",
        Op::ArrNewReg   => "arr.newr",
        Op::ArrGet      => "arr.get",
        Op::ArrSet      => "arr.set",
        Op::ArrLen      => "arr.len",
        Op::ArrFree     => "arr.free",
        Op::BitAnd      => "and",
        Op::BitOr       => "or",
        Op::BitXor      => "xor",
        Op::BitNot      => "not",
        Op::Shl         => "shl",
        Op::Shr         => "shr",
        Op::FuncPtr     => "func.ptr",
        Op::CallPtr     => "call.ptr",
        Op::CallPtrVoid => "call.ptr.void",
        Op::Panic       => "panic",
    }
}

fn instr_byte_len(instr: &Instr) -> usize {
    match instr {
        Instr::A { .. } => 4,
        Instr::B { .. } => 8,
        Instr::C { args, .. } => 8 + ((args.len() + 3) & !3),
        Instr::D { .. } => 4,
    }
}

/// imm in decoded jumps = instruction index (decode_func rewrites byte offsets -> idx)
fn instr_idx_to_label(idx: usize, offsets: &[usize]) -> String {
    match offsets.get(idx) {
        Some(byte_off) => format!(".L{idx}  ; {byte_off:#06x}"),
        None => format!(".L{idx}"),
    }
}

fn data_comment(dc: &DcFile, idx: usize) -> String {
    match dc.data.get(idx) {
        Some(DataEntry::Str(b)) => {
            let s = String::from_utf8_lossy(&b[..b.len().saturating_sub(1)]);
            let trimmed: String = s.chars().take(32).collect();
            let dots = if s.len() > 32 { "..." } else { "" };
            format!("  ; \"{}{}\"", escape_str(&trimmed), dots)
        }
        Some(DataEntry::Int(n))   => format!("  ; {n}"),
        Some(DataEntry::Float(f)) => format!("  ; {f}"),
        None => String::new(),
    }
}

fn escape_str(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn escape_char(c: char) -> String {
    match c {
        '\n' => "\\n".into(),
        '\t' => "\\t".into(),
        '\r' => "\\r".into(),
        '\'' => "\\'".into(),
        '\\' => "\\\\".into(),
        c if c.is_control() => format!("\\x{:02x}", c as u32),
        c => c.to_string(),
    }
}
