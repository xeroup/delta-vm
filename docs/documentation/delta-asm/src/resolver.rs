// checks that all referenced functions, labels, externs and data items exist

use std::collections::HashSet;
use crate::ast::*;
use crate::error::{Diagnostic, Span};

fn no_span() -> Span { Span { line: 0, col: 0 } }

pub fn resolve(program: &Program) -> Vec<Diagnostic> {
    let mut diags = Vec::new();

    let func_names: HashSet<&str> = program.funcs.iter().map(|f| f.name.as_str()).collect();
    let extern_names: HashSet<&str> = program.externs.iter().map(|e| e.name.as_str()).collect();
    let data_names: HashSet<&str> = program.data.iter().map(|d| match d {
        DataItem::Str(n, _) | DataItem::Int(n, _) | DataItem::Float(n, _) => n.as_str(),
    }).collect();

    for func in &program.funcs {
        let label_names: HashSet<&str> = func.body.iter().filter_map(|i| {
            if let Instruction::Label(n) = i { Some(n.as_str()) } else { None }
        }).collect();

        for instr in &func.body {
            check_instr(instr, &func_names, &extern_names, &data_names, &label_names, &mut diags);
        }
    }

    diags
}

fn check_instr(
    instr: &Instruction,
    funcs: &HashSet<&str>,
    externs: &HashSet<&str>,
    data: &HashSet<&str>,
    labels: &HashSet<&str>,
    diags: &mut Vec<Diagnostic>,
) {
    match instr {
        Instruction::Jmp(label) => {
            if !labels.contains(label.as_str()) {
                diags.push(Diagnostic::error(no_span(), format!("undefined label '{label}'")));
            }
        }
        Instruction::JmpIf(_, label) | Instruction::JmpIfNot(_, label) => {
            if !labels.contains(label.as_str()) {
                diags.push(Diagnostic::error(no_span(), format!("undefined label '{label}'")));
            }
        }
        Instruction::Call(_, func, _) | Instruction::CallVoid(func, _) => {
            if !funcs.contains(func.as_str()) {
                diags.push(Diagnostic::error(no_span(), format!("undefined function '{func}'")));
            }
        }
        Instruction::CallExt(func, _) | Instruction::CallExtVoid(func, _) => {
            if !externs.contains(func.as_str()) {
                diags.push(Diagnostic::error(no_span(), format!("undefined extern '{func}'")));
            }
        }
        Instruction::Load(_, op) => check_data_ref(op, data, diags),
        Instruction::Store(a, b) => {
            check_data_ref(a, data, diags);
            check_data_ref(b, data, diags);
        }
        Instruction::Alloc(_, op) => check_data_ref(op, data, diags),
        _ => {}
    }
}

fn check_data_ref(op: &Operand, data: &HashSet<&str>, diags: &mut Vec<Diagnostic>) {
    if let Operand::DataRef(name) = op {
        if !data.contains(name.as_str()) {
            diags.push(Diagnostic::error(no_span(), format!("undefined data item '@{name}'")));
        }
    }
}
