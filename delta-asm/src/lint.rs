// warnings for unused registers and unreachable code

use std::collections::HashSet;
use crate::ast::*;
use crate::error::Diagnostic;
use crate::error::Span;

fn no_span() -> Span { Span { line: 0, col: 0 } }

pub fn lint(program: &Program) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for func in &program.funcs {
        lint_unused_regs(func, &mut diags);
        lint_unreachable(func, &mut diags);
    }
    diags
}

fn lint_unused_regs(func: &FuncDecl, diags: &mut Vec<Diagnostic>) {
    let mut used: HashSet<&str> = HashSet::new();
    for instr in &func.body {
        collect_read_regs(instr, &mut used);
    }
    for local in &func.locals {
        if !used.contains(local.name.as_str()) {
            diags.push(Diagnostic::warning(
                no_span(),
                format!("in '{}': register '{}' is declared but never read", func.name, local.name),
            ));
        }
    }
}

fn collect_read_regs<'a>(instr: &'a Instruction, used: &mut HashSet<&'a str>) {
    let mut read = |op: &'a Operand| {
        if let Operand::Reg(n) = op { used.insert(n.as_str()); }
    };
    match instr {
        Instruction::Add(_, a, b) | Instruction::Sub(_, a, b) |
        Instruction::Mul(_, a, b) | Instruction::Div(_, a, b) |
        Instruction::Eq(_, a, b)  | Instruction::Ne(_, a, b)  |
        Instruction::Lt(_, a, b)  | Instruction::Le(_, a, b)  |
        Instruction::Gt(_, a, b)  | Instruction::Ge(_, a, b)  => { read(a); read(b); }
        Instruction::Load(_, op)  => read(op),
        Instruction::Alloc(_, sz) => read(sz),
        Instruction::Free(p)      => read(p),
        Instruction::Store(p, v)  => { read(p); read(v); }
        Instruction::Read(_, p)   => read(p),
        Instruction::JmpIf(c, _) | Instruction::JmpIfNot(c, _) => read(c),
        Instruction::Call(_, _, args) | Instruction::CallVoid(_, args) |
        Instruction::CallExt(_, args) | Instruction::CallExtVoid(_, args) => {
            for a in args { read(a); }
        }
        Instruction::Ret(Some(op)) => read(op),
        Instruction::Print(op) | Instruction::PrintInt(op) |
        Instruction::PrintFloat(op) | Instruction::PrintChar(op) |
        Instruction::PrintPtr(op) => read(op),
        // time instructions write into a register - treat as used to suppress unused warning
        Instruction::TimeNs(r) | Instruction::TimeMs(r) | Instruction::TimeMonoNs(r) => {
            used.insert(r.as_str());
        }
        // binary ops - read both operands
        Instruction::ModInt(_, a, b) | Instruction::ModFloat(_, a, b) |
        Instruction::PowInt(_, a, b) | Instruction::PowFloat(_, a, b) |
        Instruction::StrEq(_, a, b) | Instruction::StrCharAt(_, a, b) => { read(a); read(b); }
        // unary ops - read src
        Instruction::NegInt(_, src) | Instruction::NegFloat(_, src) |
        Instruction::AbsInt(_, src) | Instruction::AbsFloat(_, src) |
        Instruction::SqrtFloat(_, src) |
        Instruction::IntToFloat(_, src) | Instruction::FloatToInt(_, src) |
        Instruction::IntToChar(_, src) | Instruction::CharToInt(_, src) |
        Instruction::PtrToInt(_, src) |
        Instruction::StrLen(_, src) | Instruction::CharToUpper(_, src) |
        Instruction::CharToLower(_, src) | Instruction::IntToStr(_, src) |
        Instruction::FloatToStr(_, src) => read(src),
        Instruction::ArrNew(_, size) => read(size),
        Instruction::ArrGet(_, arr, idx) => { read(arr); read(idx); }
        Instruction::ArrSet(arr, idx, val) => { read(arr); read(idx); read(val); }
        Instruction::ArrLen(_, arr) => read(arr),
        Instruction::ArrFree(arr) => read(arr),
        Instruction::BitAnd(_, a, b) | Instruction::BitOr(_, a, b) |
        Instruction::BitXor(_, a, b) | Instruction::Shl(_, a, b) | Instruction::Shr(_, a, b) => { read(a); read(b); }
        Instruction::BitNot(_, a) => read(a),
        Instruction::FuncPtr(_, _) => {} // no registers read
        Instruction::CallPtr(_, fptr, args) => {
            read(fptr);
            for a in args { read(a); }
        }
        Instruction::CallPtrVoid(fptr, args) => {
            read(fptr);
            for a in args { read(a); }
        }
        Instruction::Panic(msg) => read(msg),
        _ => {}
    }
}

fn lint_unreachable(func: &FuncDecl, diags: &mut Vec<Diagnostic>) {
    let mut terminated = false;
    for instr in &func.body {
        match instr {
            Instruction::Label(_) => terminated = false,
            _ if terminated => {
                diags.push(Diagnostic::warning(
                    no_span(),
                    format!("in '{}': unreachable instruction after unconditional jump/ret", func.name),
                ));
            }
            Instruction::Ret(_) | Instruction::Jmp(_) => terminated = true,
            _ => {}
        }
    }
}
