// static type checker - verifies register types and call signatures

use std::collections::HashMap;
use crate::ast::*;
use crate::error::{Diagnostic, Span};

fn no_span() -> Span { Span { line: 0, col: 0 } }

// bool is an alias for int - they are compatible in all contexts
fn norm(t: &Type) -> &Type {
    if *t == Type::Bool { &Type::Int } else { t }
}

fn types_compat(a: &Type, b: &Type) -> bool {
    norm(a) == norm(b)
}

fn is_int_like(t: &Type) -> bool {
    matches!(t, Type::Int | Type::Bool)
}

struct Sig {
    params: Vec<Type>,
    ret: Type,
    variadic: bool,
}

pub fn check(program: &Program) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    let func_sigs: HashMap<&str, Sig> = program.funcs.iter().map(|f| (
        f.name.as_str(),
        Sig { params: f.params.iter().map(|p| p.ty.clone()).collect(), ret: f.ret_type.clone(), variadic: false }
    )).collect();

    let extern_sigs: HashMap<&str, Sig> = program.externs.iter().map(|e| (
        e.name.as_str(),
        Sig { params: e.params.clone(), ret: e.ret_type.clone(), variadic: e.variadic }
    )).collect();

    for func in &program.funcs {
        check_func(func, &func_sigs, &extern_sigs, &mut diags);
    }

    diags
}

fn check_func(
    func: &FuncDecl,
    func_sigs: &HashMap<&str, Sig>,
    extern_sigs: &HashMap<&str, Sig>,
    diags: &mut Vec<Diagnostic>,
) {
    let mut regs: HashMap<&str, Type> = HashMap::new();
    for p in &func.params { regs.insert(p.name.as_str(), p.ty.clone()); }
    for l in &func.locals { regs.insert(l.name.as_str(), l.ty.clone()); }

    for instr in &func.body {
        match instr {
            Instruction::Add(dst, a, b) | Instruction::Sub(dst, a, b) |
            Instruction::Mul(dst, a, b) | Instruction::Div(dst, a, b) => {
                let ta = op_type(a, &regs);
                let tb = op_type(b, &regs);
                if let (Some(ta), Some(tb)) = (&ta, &tb) {
                    if !is_numeric(ta) || !is_numeric(tb) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': arithmetic requires int or float operands", func.name)));
                    } else if !types_compat(ta, tb) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': arithmetic type mismatch: '{ta}' vs '{tb}'", func.name)));
                    } else if let Some(td) = regs.get(dst.as_str()) {
                        if !types_compat(td, ta) {
                            diags.push(Diagnostic::error(no_span(), format!(
                                "in '{}': dst '{dst}' is '{td}' but operands are '{ta}'", func.name)));
                        }
                    }
                }
            }

            Instruction::Eq(dst, a, b) | Instruction::Ne(dst, a, b) |
            Instruction::Lt(dst, a, b) | Instruction::Le(dst, a, b) |
            Instruction::Gt(dst, a, b) | Instruction::Ge(dst, a, b) => {
                let ta = op_type(a, &regs);
                let tb = op_type(b, &regs);
                if let (Some(ta), Some(tb)) = (&ta, &tb) {
                    if !types_compat(&ta, &tb) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': comparison type mismatch: '{ta}' vs '{tb}'", func.name)));
                    }
                }
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': comparison dst '{dst}' must be int or bool, got '{td}'", func.name)));
                    }
                }
            }

            Instruction::Load(dst, op) => {
                if let (Some(td), Some(ts)) = (regs.get(dst.as_str()), op_type(op, &regs)) {
                    if !types_compat(td, &ts) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': load type mismatch: '{dst}' is '{td}', value is '{ts}'", func.name)));
                    }
                }
            }

            Instruction::Call(dst, fname, args) => {
                if let Some(sig) = func_sigs.get(fname.as_str()) {
                    check_args(&func.name, fname, args, &sig.params, false, &regs, diags);
                    if let Some(td) = regs.get(dst.as_str()) {
                        if !types_compat(td, &sig.ret) && sig.ret != Type::Void {
                            diags.push(Diagnostic::error(no_span(), format!(
                                "in '{}': '{dst}' is '{td}' but '{fname}' returns '{}'", func.name, sig.ret)));
                        }
                    }
                }
            }

            Instruction::CallVoid(fname, args) => {
                if let Some(sig) = func_sigs.get(fname.as_str()) {
                    check_args(&func.name, fname, args, &sig.params, false, &regs, diags);
                }
            }

            Instruction::CallExt(fname, args) => {
                if let Some(sig) = extern_sigs.get(fname.as_str()) {
                    check_args(&func.name, fname, args, &sig.params, sig.variadic, &regs, diags);
                }
            }

            Instruction::CallExtVoid(fname, args) => {
                if let Some(sig) = extern_sigs.get(fname.as_str()) {
                    check_args(&func.name, fname, args, &sig.params, sig.variadic, &regs, diags);
                }
            }

            Instruction::Ret(Some(op)) => {
                if let Some(tr) = op_type(op, &regs) {
                    if !types_compat(&tr, &func.ret_type) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': returns '{}' but function declared '{}'",
                            func.name, tr, func.ret_type)));
                    }
                }
            }

            Instruction::Ret(None) => {
                if func.ret_type != Type::Void {
                    diags.push(Diagnostic::error(no_span(), format!(
                        "in '{}': bare ret in non-void function (returns '{}')",
                        func.name, func.ret_type)));
                }
            }

            Instruction::PrintInt(op) => {
                if let Some(t) = op_type(op, &regs) {
                    if !is_int_like(&t) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': printint expects int or bool, got '{t}'", func.name)));
                    }
                }
            }
            Instruction::PrintFloat(op) => {
                if let Some(t) = op_type(op, &regs) {
                    if t != Type::Float {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': printfloat expects float, got '{t}'", func.name)));
                    }
                }
            }
            Instruction::PrintChar(op) => {
                if let Some(t) = op_type(op, &regs) {
                    if t != Type::Char {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': printchar expects char, got '{t}'", func.name)));
                    }
                }
            }
            Instruction::PrintPtr(op) | Instruction::Print(op) => {
                if let Some(t) = op_type(op, &regs) {
                    if t != Type::Ptr {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': printptr expects ptr, got '{t}'", func.name)));
                    }
                }
            }

            Instruction::TimeNs(dst) | Instruction::TimeMs(dst) | Instruction::TimeMonoNs(dst) => {
                if let Some(t) = regs.get(dst.as_str()) {
                    if !is_int_like(t) {
                        diags.push(Diagnostic::error(no_span(), format!(
                            "in '{}': time instruction requires int register, got '{t}'", func.name)));
                    }
                }
            }

            Instruction::ModInt(dst, a, b) | Instruction::PowInt(dst, a, b) => {
                let ta = op_type(a, &regs); let tb = op_type(b, &regs);
                if let (Some(ta), Some(tb)) = (&ta, &tb) {
                    if !is_int_like(ta) || !is_int_like(tb) {
                        diags.push(Diagnostic::error(no_span(), format!("in '{}': instruction requires int operands", func.name)));
                    }
                }
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) {
                        diags.push(Diagnostic::error(no_span(), format!("in '{}': dst '{dst}' must be int", func.name)));
                    }
                }
            }
            Instruction::ModFloat(dst, a, b) | Instruction::PowFloat(dst, a, b) => {
                let ta = op_type(a, &regs); let tb = op_type(b, &regs);
                if let (Some(ta), Some(tb)) = (&ta, &tb) {
                    if *ta != Type::Float || *tb != Type::Float {
                        diags.push(Diagnostic::error(no_span(), format!("in '{}': instruction requires float operands", func.name)));
                    }
                }
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Float {
                        diags.push(Diagnostic::error(no_span(), format!("in '{}': dst '{dst}' must be float", func.name)));
                    }
                }
            }
            Instruction::NegInt(dst, src) | Instruction::AbsInt(dst, src) => {
                if let Some(t) = op_type(src, &regs) {
                    if !is_int_like(&t) { diags.push(Diagnostic::error(no_span(), format!("in '{}': requires int", func.name))); }
                }
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) { diags.push(Diagnostic::error(no_span(), format!("in '{}': dst must be int", func.name))); }
                }
            }
            Instruction::NegFloat(dst, src) | Instruction::AbsFloat(dst, src) | Instruction::SqrtFloat(dst, src) => {
                if let Some(t) = op_type(src, &regs) {
                    if t != Type::Float { diags.push(Diagnostic::error(no_span(), format!("in '{}': requires float", func.name))); }
                }
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Float { diags.push(Diagnostic::error(no_span(), format!("in '{}': dst must be float", func.name))); }
                }
            }
            Instruction::IntToFloat(dst, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Float { diags.push(Diagnostic::error(no_span(), format!("in '{}': itof dst must be float", func.name))); }
                }
            }
            Instruction::FloatToInt(dst, _) | Instruction::CharToInt(dst, _) | Instruction::PtrToInt(dst, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) { diags.push(Diagnostic::error(no_span(), format!("in '{}': cast dst must be int", func.name))); }
                }
            }
            Instruction::IntToChar(dst, _) | Instruction::CharToUpper(dst, _) | Instruction::CharToLower(dst, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Char { diags.push(Diagnostic::error(no_span(), format!("in '{}': cast dst must be char", func.name))); }
                }
            }
            Instruction::StrLen(dst, _) | Instruction::StrEq(dst, _, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) { diags.push(Diagnostic::error(no_span(), format!("in '{}': dst must be int", func.name))); }
                }
            }
            Instruction::StrCharAt(dst, _, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Char { diags.push(Diagnostic::error(no_span(), format!("in '{}': charat dst must be char", func.name))); }
                }
            }
            Instruction::IntToStr(dst, _) | Instruction::FloatToStr(dst, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Ptr { diags.push(Diagnostic::error(no_span(), format!("in '{}': itos/ftos dst must be ptr", func.name))); }
                }
            }

            Instruction::ArrNew(dst, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Ptr { diags.push(Diagnostic::error(no_span(), format!("in '{}': arr.new dst must be ptr", func.name))); }
                }
            }
            Instruction::ArrLen(dst, _) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) { diags.push(Diagnostic::error(no_span(), format!("in '{}': arr.len dst must be int", func.name))); }
                }
            }
            Instruction::ArrGet(dst, _, _) => { let _ = dst; }
            Instruction::ArrSet(_, _, _) | Instruction::ArrFree(_) => {}

            Instruction::ReadChar(dst) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Char { diags.push(Diagnostic::error(no_span(), format!("in '{}': readchar dst must be char", func.name))); }
                }
            }
            Instruction::ReadInt(dst) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) { diags.push(Diagnostic::error(no_span(), format!("in '{}': readint dst must be int", func.name))); }
                }
            }
            Instruction::ReadFloat(dst) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Float { diags.push(Diagnostic::error(no_span(), format!("in '{}': readfloat dst must be float", func.name))); }
                }
            }
            Instruction::ReadLine(dst) => {
                if let Some(td) = regs.get(dst.as_str()) {
                    if *td != Type::Ptr { diags.push(Diagnostic::error(no_span(), format!("in '{}': readline dst must be ptr", func.name))); }
                }
            }

            Instruction::BitAnd(dst, a, b) | Instruction::BitOr(dst, a, b) |
            Instruction::BitXor(dst, a, b) | Instruction::Shl(dst, a, b) | Instruction::Shr(dst, a, b) => {
                for op in [a, b] {
                    if let Some(t) = op_type(op, &regs) {
                        if !is_int_like(&t) { diags.push(Diagnostic::error(no_span(), format!("in '{}': bitwise operands must be int", func.name))); }
                    }
                }
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) { diags.push(Diagnostic::error(no_span(), format!("in '{}': bitwise dst must be int", func.name))); }
                }
            }
            Instruction::BitNot(dst, a) => {
                if let Some(t) = op_type(a, &regs) {
                    if !is_int_like(&t) { diags.push(Diagnostic::error(no_span(), format!("in '{}': not operand must be int", func.name))); }
                }
                if let Some(td) = regs.get(dst.as_str()) {
                    if !is_int_like(td) { diags.push(Diagnostic::error(no_span(), format!("in '{}': not dst must be int", func.name))); }
                }
            }

            _ => {}
        }
    }
}

fn check_args(
    caller: &str, callee: &str,
    args: &[Operand], params: &[Type],
    variadic: bool,
    regs: &HashMap<&str, Type>,
    diags: &mut Vec<Diagnostic>,
) {
    let min = params.len();
    if variadic {
        if args.len() < min {
            diags.push(Diagnostic::error(no_span(), format!(
                "in '{caller}': '{callee}' expects at least {min} args, got {}", args.len())));
            return;
        }
    } else if args.len() != min {
        diags.push(Diagnostic::error(no_span(), format!(
            "in '{caller}': '{callee}' expects {min} args, got {}", args.len())));
        return;
    }
    for (i, (arg, expected)) in args.iter().zip(params.iter()).enumerate() {
        if let Some(got) = op_type(arg, regs) {
            if !types_compat(&got, expected) {
                diags.push(Diagnostic::error(no_span(), format!(
                    "in '{caller}': arg {i} to '{callee}' expects '{expected}', got '{got}'")));
            }
        }
    }
}

fn op_type(op: &Operand, regs: &HashMap<&str, Type>) -> Option<Type> {
    match op {
        Operand::Reg(n) => regs.get(n.as_str()).cloned(),
        Operand::ImmInt(_) => Some(Type::Int),
        Operand::ImmFloat(_) => Some(Type::Float),
        Operand::ImmChar(_) => Some(Type::Char),
        Operand::DataRef(_) => Some(Type::Ptr),
    }
}

fn is_numeric(t: &Type) -> bool {
    matches!(t, Type::Int | Type::Bool | Type::Float)
}

impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Bool => write!(f, "bool"),
            Type::Float => write!(f, "float"),
            Type::Char => write!(f, "char"),
            Type::Ptr => write!(f, "ptr"),
            Type::Void => write!(f, "void"),
        }
    }
}
