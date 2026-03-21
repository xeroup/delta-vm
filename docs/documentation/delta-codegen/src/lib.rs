// compiles a delta-asm Program AST into a DcFile (bytecode)

use std::collections::HashMap;

use delta_asm::ast::{
    DataItem, FuncDecl, Instruction, Operand, Program, Type,
};
use delta_format::{
    encoding::{f32_bits, Instr},
    file::{DataEntry, DcFile, ExternEntry, FuncEntry},
    opcode::Op,
};

#[derive(Debug)]
pub struct CodegenError(pub String);

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "codegen error: {}", self.0)
    }
}

type Result<T> = std::result::Result<T, CodegenError>;

pub fn compile(program: &Program) -> Result<DcFile> {
    let mut dc = DcFile::default();

    // build data section
    for item in &program.data {
        match item {
            DataItem::Str(_, s) => {
                let mut bytes = s.as_bytes().to_vec();
                bytes.push(0); // null-terminate
                dc.data.push(DataEntry::Str(bytes));
            }
            DataItem::Int(_, n) => dc.data.push(DataEntry::Int(*n)),
            DataItem::Float(_, f) => dc.data.push(DataEntry::Float(*f)),
        }
    }

    // build data name -> index map
    let data_index: HashMap<&str, u32> = program.data.iter().enumerate()
        .map(|(i, d)| {
            let name = match d {
                DataItem::Str(n, _) | DataItem::Int(n, _) | DataItem::Float(n, _) => n.as_str(),
            };
            (name, i as u32)
        })
        .collect();

    // build extern name -> index map and extern section
    let extern_index: HashMap<&str, u16> = program.externs.iter().enumerate()
        .map(|(i, e)| (e.name.as_str(), i as u16))
        .collect();
    for ext in &program.externs {
        dc.externs.push(ExternEntry {
            name: ext.name.clone(),
            param_count: ext.params.len() as u8,
            variadic: ext.variadic,
        });
    }

    // build func name -> index map
    let func_index: HashMap<&str, u16> = program.funcs.iter().enumerate()
        .map(|(i, f)| (f.name.as_str(), i as u16))
        .collect();

    // compile each function
    for func in &program.funcs {
        let code_offset = dc.code.len() as u32;
        let func_code = compile_func(func, &func_index, &extern_index, &data_index)?;
        let code_len = func_code.len() as u32;
        dc.code.extend(func_code);

        let reg_count = (func.params.len() + func.locals.len() + 1) as u8; // +1 for scratch
        dc.funcs.push(FuncEntry {
            code_offset,
            code_len,
            reg_count,
            param_count: func.params.len() as u8,
            name: func.name.clone(),
        });
    }

    Ok(dc)
}

fn compile_func(
    func: &FuncDecl,
    func_index: &HashMap<&str, u16>,
    extern_index: &HashMap<&str, u16>,
    data_index: &HashMap<&str, u32>,
) -> Result<Vec<u8>> {
    // build register name -> index map (params first, then locals)
    let mut reg_index: HashMap<&str, u8> = HashMap::new();
    for (i, p) in func.params.iter().enumerate() {
        reg_index.insert(p.name.as_str(), i as u8);
    }
    for (i, l) in func.locals.iter().enumerate() {
        reg_index.insert(l.name.as_str(), (func.params.len() + i) as u8);
    }
    // scratch register - one slot after all declared registers, for immediate operands
    let scratch: u8 = (func.params.len() + func.locals.len()) as u8;

    // build type map for registers
    let mut reg_type: HashMap<&str, &Type> = HashMap::new();
    for p in &func.params { reg_type.insert(p.name.as_str(), &p.ty); }
    for l in &func.locals { reg_type.insert(l.name.as_str(), &l.ty); }

    // first pass: collect label positions (byte offsets)
    // we need two passes because jumps reference labels that may be ahead
    let mut label_pos: HashMap<&str, u32> = HashMap::new();
    {
        let mut offset: u32 = 0;
        for instr in &func.body {
            match instr {
                Instruction::Label(name) => { label_pos.insert(name.as_str(), offset); }
                _ => { offset += instr_size(instr); }
            }
        }
    }

    // macro shorthand: emit_operand with scratch and buf in scope

    let mut buf = Vec::new();
    macro_rules! eo {
        ($op:expr) => { emit_operand($op, &reg_index, scratch, &mut buf)? }
    }
    for instr in &func.body {
        match instr {
            Instruction::Label(_) => {} // labels emit no code

            // arithmetic (type determined by dst)
            Instruction::Add(dst, a, b) => {
                let op = if reg_type.get(dst.as_str()) == Some(&&Type::Float) { Op::AddFloat } else { Op::AddInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Sub(dst, a, b) => {
                let op = if reg_type.get(dst.as_str()) == Some(&&Type::Float) { Op::SubFloat } else { Op::SubInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Mul(dst, a, b) => {
                let op = if reg_type.get(dst.as_str()) == Some(&&Type::Float) { Op::MulFloat } else { Op::MulInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Div(dst, a, b) => {
                let op = if reg_type.get(dst.as_str()) == Some(&&Type::Float) { Op::DivFloat } else { Op::DivInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }

            // comparisons
            Instruction::Eq(dst, a, b) => {
                let op = match operand_type(a, &reg_type) { Some(Type::Float) => Op::EqFloat, Some(Type::Char) => Op::EqChar, _ => Op::EqInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Ne(dst, a, b) => {
                let op = match operand_type(a, &reg_type) { Some(Type::Float) => Op::NeFloat, Some(Type::Char) => Op::NeChar, _ => Op::NeInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Lt(dst, a, b) => {
                let op = if operand_type(a, &reg_type) == Some(Type::Float) { Op::LtFloat } else { Op::LtInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Le(dst, a, b) => {
                let op = if operand_type(a, &reg_type) == Some(Type::Float) { Op::LeFloat } else { Op::LeInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Gt(dst, a, b) => {
                let op = if operand_type(a, &reg_type) == Some(Type::Float) { Op::GtFloat } else { Op::GtInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }
            Instruction::Ge(dst, a, b) => {
                let op = if operand_type(a, &reg_type) == Some(Type::Float) { Op::GeFloat } else { Op::GeInt };
                let (ra, rb) = (eo!(a), eo!(b));
                Instr::A { op, dst: reg(dst, &reg_index)?, a: ra, b: rb }.encode(&mut buf);
            }

            Instruction::Load(dst, op) => {
                let d = reg(dst, &reg_index)?;
                match op {
                    Operand::ImmInt(n)   => Instr::B { op: Op::LoadInt,   dst: d, imm: *n as u32 }.encode(&mut buf),
                    Operand::ImmFloat(f) => Instr::B { op: Op::LoadFloat, dst: d, imm: f32_bits(*f) }.encode(&mut buf),
                    Operand::ImmChar(c)  => Instr::B { op: Op::LoadChar,  dst: d, imm: *c as u32 }.encode(&mut buf),
                    Operand::DataRef(name) => {
                        let idx = data_index.get(name.as_str()).copied()
                            .ok_or_else(|| CodegenError(format!("undefined data ref '@{name}'")))?;
                        Instr::B { op: Op::LoadPtr, dst: d, imm: idx }.encode(&mut buf);
                    }
                    Operand::Reg(src) => {
                        let s = reg(src, &reg_index)?;
                        Instr::A { op: Op::AddInt, dst: d, a: s, b: s }.encode(&mut buf);
                    }
                }
            }

            Instruction::Alloc(dst, size) => {
                let d = reg(dst, &reg_index)?;
                match size {
                    Operand::ImmInt(n) => Instr::B { op: Op::Alloc, dst: d, imm: *n as u32 }.encode(&mut buf),
                    _ => { let ra = eo!(size); Instr::A { op: Op::AllocReg, dst: d, a: ra, b: 0 }.encode(&mut buf); }
                }
            }
            Instruction::Free(ptr)       => { let r = eo!(ptr);        Instr::D { op: Op::Free,  src: r }.encode(&mut buf); }
            Instruction::Store(ptr, val) => { let (rp, rv) = (eo!(ptr), eo!(val)); Instr::A { op: Op::Store, dst: 0, a: rp, b: rv }.encode(&mut buf); }
            Instruction::Read(dst, ptr)  => { let rp = eo!(ptr);       Instr::A { op: Op::Read,  dst: reg(dst, &reg_index)?, a: rp, b: 0 }.encode(&mut buf); }

            Instruction::Jmp(label) => {
                Instr::B { op: Op::Jmp, dst: 0, imm: label_offset(label, &label_pos)? }.encode(&mut buf);
            }
            Instruction::JmpIf(cond, label) => {
                let rc = eo!(cond);
                Instr::B { op: Op::JmpIf, dst: rc, imm: label_offset(label, &label_pos)? }.encode(&mut buf);
            }
            Instruction::JmpIfNot(cond, label) => {
                let rc = eo!(cond);
                Instr::B { op: Op::JmpIfNot, dst: rc, imm: label_offset(label, &label_pos)? }.encode(&mut buf);
            }

            Instruction::Call(dst, fname, args) => {
                let arg_regs = args_to_regs_with_scratch(args, &reg_index, scratch, &mut buf)?;
                Instr::C { op: Op::Call, dst: reg(dst, &reg_index)?, func_idx: func_idx(fname, func_index)?, args: arg_regs }.encode(&mut buf);
            }
            Instruction::CallVoid(fname, args) => {
                let arg_regs = args_to_regs_with_scratch(args, &reg_index, scratch, &mut buf)?;
                Instr::C { op: Op::CallVoid, dst: 0, func_idx: func_idx(fname, func_index)?, args: arg_regs }.encode(&mut buf);
            }
            Instruction::CallExt(fname, args) => {
                let arg_regs = args_to_regs_with_scratch(args, &reg_index, scratch, &mut buf)?;
                Instr::C { op: Op::CallExt, dst: 0, func_idx: ext_idx(fname, extern_index)?, args: arg_regs }.encode(&mut buf);
            }
            Instruction::CallExtVoid(fname, args) => {
                let arg_regs = args_to_regs_with_scratch(args, &reg_index, scratch, &mut buf)?;
                Instr::C { op: Op::CallExtVoid, dst: 0, func_idx: ext_idx(fname, extern_index)?, args: arg_regs }.encode(&mut buf);
            }

            Instruction::Ret(Some(op)) => { let r = eo!(op); Instr::D { op: Op::Ret, src: r }.encode(&mut buf); }
            Instruction::Ret(None)     => { Instr::D { op: Op::RetVoid, src: 0 }.encode(&mut buf); }

            Instruction::Print(op) | Instruction::PrintPtr(op) => { let r = eo!(op); Instr::D { op: Op::PrintPtr,   src: r }.encode(&mut buf); }
            Instruction::PrintInt(op)   => { let r = eo!(op); Instr::D { op: Op::PrintInt,   src: r }.encode(&mut buf); }
            Instruction::PrintFloat(op) => { let r = eo!(op); Instr::D { op: Op::PrintFloat, src: r }.encode(&mut buf); }
            Instruction::PrintChar(op)  => { let r = eo!(op); Instr::D { op: Op::PrintChar,  src: r }.encode(&mut buf); }

            Instruction::TimeNs(dst)     => { Instr::D { op: Op::TimeNs,     src: reg(dst, &reg_index)? }.encode(&mut buf); }
            Instruction::TimeMs(dst)     => { Instr::D { op: Op::TimeMs,     src: reg(dst, &reg_index)? }.encode(&mut buf); }
            Instruction::TimeMonoNs(dst) => { Instr::D { op: Op::TimeMonoNs, src: reg(dst, &reg_index)? }.encode(&mut buf); }

            // extended arithmetic
            Instruction::ModInt(dst, a, b)   => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::ModInt,   dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::ModFloat(dst, a, b) => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::ModFloat, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::PowInt(dst, a, b)   => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::PowInt,   dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::PowFloat(dst, a, b) => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::PowFloat, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::NegInt(dst, src)    => { let ra=eo!(src); Instr::A { op: Op::NegInt,   dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::NegFloat(dst, src)  => { let ra=eo!(src); Instr::A { op: Op::NegFloat, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::AbsInt(dst, src)    => { let ra=eo!(src); Instr::A { op: Op::AbsInt,   dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::AbsFloat(dst, src)  => { let ra=eo!(src); Instr::A { op: Op::AbsFloat, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::SqrtFloat(dst, src) => { let ra=eo!(src); Instr::A { op: Op::SqrtFloat,dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }

            // type casts
            Instruction::IntToFloat(dst, src) => { let ra=eo!(src); Instr::A { op: Op::IntToFloat, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::FloatToInt(dst, src) => { let ra=eo!(src); Instr::A { op: Op::FloatToInt, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::IntToChar(dst, src)  => { let ra=eo!(src); Instr::A { op: Op::IntToChar,  dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::CharToInt(dst, src)  => { let ra=eo!(src); Instr::A { op: Op::CharToInt,  dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::PtrToInt(dst, src)   => { let ra=eo!(src); Instr::A { op: Op::PtrToInt,   dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }

            // string / char ops
            Instruction::StrLen(dst, src)        => { let ra=eo!(src);         Instr::A { op: Op::StrLen,     dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::StrEq(dst, a, b)        => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::StrEq,  dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::StrCharAt(dst, s, idx)  => { let (ra,rb)=(eo!(s),eo!(idx)); Instr::A { op: Op::StrCharAt, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::CharToUpper(dst, src)   => { let ra=eo!(src); Instr::A { op: Op::CharToUpper, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::CharToLower(dst, src)   => { let ra=eo!(src); Instr::A { op: Op::CharToLower, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::IntToStr(dst, src)      => { let ra=eo!(src); Instr::A { op: Op::IntToStr,    dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::FloatToStr(dst, src)    => { let ra=eo!(src); Instr::A { op: Op::FloatToStr,  dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }

            // arrays
            Instruction::ArrNew(dst, size) => {
                let d = reg(dst, &reg_index)?;
                match size {
                    Operand::ImmInt(n) => Instr::B { op: Op::ArrNew, dst: d, imm: *n as u32 }.encode(&mut buf),
                    _ => { let ra=eo!(size); Instr::A { op: Op::ArrNewReg, dst: d, a: ra, b: 0 }.encode(&mut buf); }
                }
            }
            Instruction::ArrGet(dst, arr, idx)  => { let (ra,rb)=(eo!(arr),eo!(idx)); Instr::A { op: Op::ArrGet, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::ArrSet(arr, idx, val)  => { let (rd,ra,rb)=(eo!(arr),eo!(idx),eo!(val)); Instr::A { op: Op::ArrSet, dst:rd, a:ra, b:rb }.encode(&mut buf); }
            Instruction::ArrLen(dst, arr)       => { let ra=eo!(arr); Instr::A { op: Op::ArrLen, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::ArrFree(arr)           => { let r=eo!(arr);  Instr::D { op: Op::ArrFree, src: r }.encode(&mut buf); }

            // stdin input
            Instruction::ReadChar(dst)  => { Instr::D { op: Op::ReadChar,  src: reg(dst,&reg_index)? }.encode(&mut buf); }
            Instruction::ReadInt(dst)   => { Instr::D { op: Op::ReadInt,   src: reg(dst,&reg_index)? }.encode(&mut buf); }
            Instruction::ReadFloat(dst) => { Instr::D { op: Op::ReadFloat, src: reg(dst,&reg_index)? }.encode(&mut buf); }
            Instruction::ReadLine(dst)  => { Instr::D { op: Op::ReadLine,  src: reg(dst,&reg_index)? }.encode(&mut buf); }

            // bitwise
            Instruction::BitAnd(dst, a, b) => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::BitAnd, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::BitOr(dst, a, b)  => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::BitOr,  dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::BitXor(dst, a, b) => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::BitXor, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::BitNot(dst, src)  => { let ra=eo!(src); Instr::A { op: Op::BitNot, dst: reg(dst,&reg_index)?, a:ra, b:0 }.encode(&mut buf); }
            Instruction::Shl(dst, a, b)    => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::Shl, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }
            Instruction::Shr(dst, a, b)    => { let (ra,rb)=(eo!(a),eo!(b)); Instr::A { op: Op::Shr, dst: reg(dst,&reg_index)?, a:ra, b:rb }.encode(&mut buf); }

            // function pointers
            Instruction::FuncPtr(dst, fname) => {
                let fidx = func_idx(fname, func_index)?;
                Instr::B { op: Op::FuncPtr, dst: reg(dst, &reg_index)?, imm: fidx as u32 }.encode(&mut buf);
            }
            Instruction::CallPtr(dst, fptr, args) => {
                // encode as format C: func_idx field holds the register index of the function pointer
                let fptr_reg = eo!(fptr);
                let arg_regs = args_to_regs_with_scratch(args, &reg_index, scratch, &mut buf)?;
                Instr::C { op: Op::CallPtr, dst: reg(dst, &reg_index)?, func_idx: fptr_reg as u16, args: arg_regs }.encode(&mut buf);
            }
            Instruction::CallPtrVoid(fptr, args) => {
                let fptr_reg = eo!(fptr);
                let arg_regs = args_to_regs_with_scratch(args, &reg_index, scratch, &mut buf)?;
                Instr::C { op: Op::CallPtrVoid, dst: 0, func_idx: fptr_reg as u16, args: arg_regs }.encode(&mut buf);
            }
            Instruction::Panic(msg) => {
                let r = eo!(msg);
                Instr::D { op: Op::Panic, src: r }.encode(&mut buf);
            }
        }
    }

    Ok(buf)
}

// returns the byte size an instruction will emit (for label offset calculation)
fn instr_size(instr: &Instruction) -> u32 {
    match instr {
        Instruction::Label(_) => 0,
        // format B (8 bytes)
        Instruction::Load(_, Operand::ImmInt(_)) |
        Instruction::Load(_, Operand::ImmFloat(_)) |
        Instruction::Load(_, Operand::ImmChar(_)) |
        Instruction::Load(_, Operand::DataRef(_)) |
        Instruction::Alloc(_, Operand::ImmInt(_)) |
        Instruction::ArrNew(_, Operand::ImmInt(_)) |
        Instruction::FuncPtr(_, _) |
        Instruction::Jmp(_) => 8,
        // jmpif/jmpifnot: 8 bytes + possible load for cond immediate (rare but possible)
        Instruction::JmpIf(cond, _) | Instruction::JmpIfNot(cond, _) => {
            8 + imm_load_size(cond)
        }
        // format C (variable) - base 8 + args padded to 4 + possible imm loads for each arg
        Instruction::Call(_, _, args) | Instruction::CallVoid(_, args) |
        Instruction::CallExt(_, args) | Instruction::CallExtVoid(_, args) => {
            let imm_loads: u32 = args.iter().map(imm_load_size).sum();
            imm_loads + 8 + ((args.len() + 3) & !3) as u32
        }
        Instruction::CallPtr(_, fptr, args) | Instruction::CallPtrVoid(fptr, args) => {
            let imm_loads: u32 = args.iter().map(imm_load_size).sum();
            imm_load_size(fptr) + imm_loads + 8 + ((args.len() + 3) & !3) as u32
        }
        // format D (4 bytes) - these take only register operands or none
        Instruction::Free(_) | Instruction::Ret(_) |
        Instruction::Print(_) | Instruction::PrintInt(_) |
        Instruction::PrintFloat(_) | Instruction::PrintChar(_) | Instruction::PrintPtr(_) |
        Instruction::TimeNs(_) | Instruction::TimeMs(_) | Instruction::TimeMonoNs(_) |
        Instruction::ArrFree(_) |
        Instruction::ReadChar(_) | Instruction::ReadInt(_) |
        Instruction::ReadFloat(_) | Instruction::ReadLine(_) |
        Instruction::Panic(_) => 4,
        // format A + possible preceding loads for immediate operands
        _ => 4 + instr_imm_loads(instr),
    }
}

/// Returns extra bytes needed to load immediate operands into scratch before a format-A instruction.
fn instr_imm_loads(instr: &Instruction) -> u32 {
    match instr {
        // binary ops: both a and b can be immediate -> up to 2x8 bytes
        Instruction::Add(_, a, b) | Instruction::Sub(_, a, b) |
        Instruction::Mul(_, a, b) | Instruction::Div(_, a, b) |
        Instruction::Eq(_, a, b)  | Instruction::Ne(_, a, b)  |
        Instruction::Lt(_, a, b)  | Instruction::Le(_, a, b)  |
        Instruction::Gt(_, a, b)  | Instruction::Ge(_, a, b)  |
        Instruction::ModInt(_, a, b) | Instruction::ModFloat(_, a, b) |
        Instruction::PowInt(_, a, b) | Instruction::PowFloat(_, a, b) |
        Instruction::StrEq(_, a, b) | Instruction::StrCharAt(_, a, b) |
        Instruction::ArrGet(_, a, b) |
        Instruction::BitAnd(_, a, b) | Instruction::BitOr(_, a, b) |
        Instruction::BitXor(_, a, b) | Instruction::Shl(_, a, b) | Instruction::Shr(_, a, b) =>
            imm_load_size(a) + imm_load_size(b),
        // unary ops: one source can be immediate
        Instruction::NegInt(_, s) | Instruction::NegFloat(_, s) |
        Instruction::AbsInt(_, s) | Instruction::AbsFloat(_, s) |
        Instruction::SqrtFloat(_, s) |
        Instruction::IntToFloat(_, s) | Instruction::FloatToInt(_, s) |
        Instruction::IntToChar(_, s) | Instruction::CharToInt(_, s) | Instruction::PtrToInt(_, s) |
        Instruction::StrLen(_, s) | Instruction::CharToUpper(_, s) | Instruction::CharToLower(_, s) |
        Instruction::IntToStr(_, s) | Instruction::FloatToStr(_, s) |
        Instruction::ArrLen(_, s) | Instruction::BitNot(_, s) | Instruction::ArrFree(s) |
        Instruction::Free(s) | Instruction::Read(_, s) =>
            imm_load_size(s),
        Instruction::Store(p, v) => imm_load_size(p) + imm_load_size(v),
        Instruction::ArrSet(a, b, c) => imm_load_size(a) + imm_load_size(b) + imm_load_size(c),
        Instruction::Alloc(_, Operand::ImmInt(_)) => 0, // handled as format B
        Instruction::Alloc(_, s) => imm_load_size(s),
        Instruction::ArrNew(_, Operand::ImmInt(_)) => 0, // format B
        Instruction::ArrNew(_, s) => imm_load_size(s),
        Instruction::Panic(s) => imm_load_size(s),
        _ => 0,
    }
}

fn imm_load_size(op: &Operand) -> u32 {
    match op {
        Operand::ImmInt(_) | Operand::ImmFloat(_) | Operand::ImmChar(_) => 8,
        _ => 0,
    }
}

fn reg(name: &str, map: &HashMap<&str, u8>) -> Result<u8> {
    map.get(name).copied().ok_or_else(|| CodegenError(format!("unknown register '{name}'")))
}

/// Resolve an operand to a register index.
/// If the operand is an immediate, emit a load into `scratch` and return `scratch`.
fn emit_operand(op: &Operand, map: &HashMap<&str, u8>, scratch: u8, buf: &mut Vec<u8>) -> Result<u8> {
    match op {
        Operand::Reg(n) => reg(n, map),
        Operand::ImmInt(n) => {
            Instr::B { op: Op::LoadInt, dst: scratch, imm: *n as u32 }.encode(buf);
            Ok(scratch)
        }
        Operand::ImmFloat(f) => {
            Instr::B { op: Op::LoadFloat, dst: scratch, imm: f32_bits(*f) }.encode(buf);
            Ok(scratch)
        }
        Operand::ImmChar(c) => {
            Instr::B { op: Op::LoadChar, dst: scratch, imm: *c as u32 }.encode(buf);
            Ok(scratch)
        }
        Operand::DataRef(name) => {
            Err(CodegenError(format!("data ref '@{name}' cannot be used as a source operand here")))
        }
    }
}

/// Like emit_operand but only accepts registers (no immediates) - for dst positions.
fn operand_reg(op: &Operand, map: &HashMap<&str, u8>) -> Result<u8> {
    match op {
        Operand::Reg(n) => reg(n, map),
        _ => Err(CodegenError("expected register operand, got immediate".into())),
    }
}

/// Emit load instructions for any immediate args, using scratch slots starting at `scratch`.
/// Returns the register indices to use for each arg.
/// Each immediate gets its own slot (scratch, scratch+1, scratch+2, ...) to avoid clobbering.
fn args_to_regs_with_scratch(args: &[Operand], map: &HashMap<&str, u8>, scratch: u8, buf: &mut Vec<u8>) -> Result<Vec<u8>> {
    // scratch and scratch+1 are the two available scratch slots.
    // Each immediate argument alternates between them so that two immediates
    // in the same call don't clobber each other.
    let mut out = Vec::with_capacity(args.len());
    let mut slot_toggle = 0u8; // 0 -> scratch, 1 -> scratch+1
    for a in args {
        match a {
            Operand::Reg(n) => out.push(reg(n, map)?),
            Operand::ImmInt(n) => {
                let s = scratch + slot_toggle;
                slot_toggle ^= 1;
                Instr::B { op: Op::LoadInt, dst: s, imm: *n as u32 }.encode(buf);
                out.push(s);
            }
            Operand::ImmFloat(f) => {
                let s = scratch + slot_toggle;
                slot_toggle ^= 1;
                Instr::B { op: Op::LoadFloat, dst: s, imm: f32_bits(*f) }.encode(buf);
                out.push(s);
            }
            Operand::ImmChar(c) => {
                let s = scratch + slot_toggle;
                slot_toggle ^= 1;
                Instr::B { op: Op::LoadChar, dst: s, imm: *c as u32 }.encode(buf);
                out.push(s);
            }
            Operand::DataRef(name) => {
                return Err(CodegenError(format!("data ref '@{name}' cannot be used as call argument")));
            }
        }
    }
    Ok(out)
}

fn operand_type<'a>(op: &Operand, reg_type: &HashMap<&str, &'a Type>) -> Option<Type> {
    match op {
        Operand::Reg(n) => reg_type.get(n.as_str()).map(|t| (*t).clone()),
        Operand::ImmInt(_) => Some(Type::Int),
        Operand::ImmFloat(_) => Some(Type::Float),
        Operand::ImmChar(_) => Some(Type::Char),
        Operand::DataRef(_) => Some(Type::Ptr),
    }
}

fn func_idx(name: &str, map: &HashMap<&str, u16>) -> Result<u16> {
    map.get(name).copied().ok_or_else(|| CodegenError(format!("unknown function '{name}'")))
}

fn ext_idx(name: &str, map: &HashMap<&str, u16>) -> Result<u16> {
    map.get(name).copied().ok_or_else(|| CodegenError(format!("unknown extern '{name}'")))
}

fn label_offset(label: &str, map: &HashMap<&str, u32>) -> Result<u32> {
    map.get(label).copied().ok_or_else(|| CodegenError(format!("unknown label '{label}'")))
}

fn args_to_regs(args: &[Operand], map: &HashMap<&str, u8>) -> Result<Vec<u8>> {
    args.iter().map(|a| operand_reg(a, map)).collect()
}

/// Count max number of immediate operands in a single call instruction across the whole function.
/// Used to pre-allocate enough scratch register slots.

#[cfg(test)]
mod tests {
    use super::*;
    use delta_asm::parse;

    #[test]
    fn test_compile_simple_func() {
        let src = r#"
.func add(int r0, int r1) -> int
    int r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let dc = compile(&prog).unwrap();
        assert_eq!(dc.funcs.len(), 1);
        assert_eq!(dc.funcs[0].name, "add");
        assert_eq!(dc.funcs[0].param_count, 2);
        assert_eq!(dc.funcs[0].reg_count, 4); // 2 params + 1 local + 1 scratch
        assert!(!dc.code.is_empty());
    }

    #[test]
    fn test_compile_data_section() {
        let src = r#"
.section data
    .str msg "hello"

.section code
.func main() -> int
    ptr r0
    int r1
    load r0, @msg
    load r1, 0
    ret r1
.endfunc
"#;
        let prog = parse(src).unwrap();
        let dc = compile(&prog).unwrap();
        assert_eq!(dc.data.len(), 1);
        match &dc.data[0] {
            DataEntry::Str(b) => assert_eq!(b, b"hello\0"),
            _ => panic!("expected Str data entry"),
        }
    }

    #[test]
    fn test_compile_extern() {
        let src = r#"
.extern putchar(char) -> int

.section code
.func main() -> int
    char r0
    int r1
    load r0, 'A'
    call.ext putchar, r0
    load r1, 0
    ret r1
.endfunc
"#;
        let prog = parse(src).unwrap();
        let dc = compile(&prog).unwrap();
        assert_eq!(dc.externs.len(), 1);
        assert_eq!(dc.externs[0].name, "putchar");
    }

    #[test]
    fn test_compile_print_instructions() {
        let src = r#"
.func main() -> int
    int r0
    float r1
    char r2
    load r0, 42
    load r1, 3.14
    load r2, 'X'
    printint r0
    printfloat r1
    printchar r2
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let dc = compile(&prog).unwrap();
        assert!(!dc.code.is_empty());
        // roundtrip through serialize/deserialize
        let bytes = dc.serialize();
        let loaded = delta_format::file::DcFile::deserialize(&bytes).unwrap();
        assert_eq!(loaded.funcs.len(), 1);
    }

    #[test]
    fn test_compile_jumps_and_labels() {
        let src = r#"
.func countdown(int r0) -> int
    int r1
    load r1, 0
loop
    eq r1, r0, r1
    jmpif r1, done
    sub r0, r0, r1
    jmp loop
done
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let dc = compile(&prog).unwrap();
        assert!(!dc.code.is_empty());
    }

    #[test]
    fn test_compile_call() {
        let src = r#"
.func double(int r0) -> int
    int r1
    add r1, r0, r0
    ret r1
.endfunc

.func main() -> int
    int r0
    int r1
    load r0, 5
    call r1, double, r0
    ret r1
.endfunc
"#;
        let prog = parse(src).unwrap();
        let dc = compile(&prog).unwrap();
        assert_eq!(dc.funcs.len(), 2);
    }

    #[test]
    fn test_compile_roundtrip() {
        let src = r#"
.section data
    .str greeting "Hello"
    .i64 answer 42

.section code
.func main() -> int
    ptr r0
    int r1
    load r0, @greeting
    load r1, @answer
    printint r1
    ret r1
.endfunc
"#;
        let prog = parse(src).unwrap();
        let dc = compile(&prog).unwrap();
        let bytes = dc.serialize();
        let loaded = delta_format::file::DcFile::deserialize(&bytes).unwrap();
        assert_eq!(loaded.funcs.len(), 1);
        assert_eq!(loaded.data.len(), 2);
    }
}
