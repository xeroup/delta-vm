pub mod ast;
pub mod checker;
pub mod error;
pub mod lexer;
pub mod lint;
pub mod parser;
pub mod resolver;

use lexer::Lexer;
use parser::Parser;

pub use ast::Program;
pub use error::{AsmError, Diagnostic, Result};

pub fn parse(src: &str) -> Result<Program> {
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize()?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}

pub fn analyze(program: &Program) -> (Vec<Diagnostic>, Vec<Diagnostic>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for d in resolver::resolve(program) {
        match d.severity {
            error::Severity::Error => errors.push(d),
            error::Severity::Warning => warnings.push(d),
        }
    }
    for d in checker::check(program) {
        match d.severity {
            error::Severity::Error => errors.push(d),
            error::Severity::Warning => warnings.push(d),
        }
    }
    for d in lint::lint(program) {
        match d.severity {
            error::Severity::Error => errors.push(d),
            error::Severity::Warning => warnings.push(d),
        }
    }
    (errors, warnings)
}

pub fn parse_and_analyze(src: &str) -> Option<Program> {
    let program = match parse(src) {
        Ok(p) => p,
        Err(e) => { eprintln!("{e}"); return None; }
    };
    let (errors, warnings) = analyze(&program);
    for w in &warnings { eprintln!("{w}"); }
    for e in &errors { eprintln!("{e}"); }
    if errors.is_empty() { Some(program) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_simple_func() {
        let src = r#"
.func add(int r0, int r1) -> int
    int r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        assert_eq!(prog.funcs.len(), 1);
        assert_eq!(prog.funcs[0].name, "add");
        assert_eq!(prog.funcs[0].params.len(), 2);
        assert_eq!(prog.funcs[0].locals.len(), 1);
    }

    #[test]
    fn test_extern_and_data() {
        let src = r#"
.extern putchar(char) -> int

.section data
    .str greeting "hello\n"
    .i64 answer 42

.section code
.func main() -> int
    int r0
    load r0, 0
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        assert_eq!(prog.externs.len(), 1);
        assert_eq!(prog.data.len(), 2);
        assert_eq!(prog.funcs.len(), 1);
    }

    #[test]
    fn test_all_arithmetic() {
        let src = r#"
.func math(int r0, int r1) -> int
    int r2
    int r3
    int r4
    int r5
    add r2, r0, r1
    sub r3, r0, r1
    mul r4, r0, r1
    div r5, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        assert_eq!(prog.funcs[0].locals.len(), 4);
        assert_eq!(prog.funcs[0].body.len(), 5);
    }

    #[test]
    fn test_comparisons_and_jumps() {
        let src = r#"
.func cmp(int r0, int r1) -> int
    int r2
    eq r2, r0, r1
    jmpif r2, done
    load r2, 0
done
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let body = &prog.funcs[0].body;
        assert!(matches!(body[0], Instruction::Eq(_, _, _)));
        assert!(matches!(body[1], Instruction::JmpIf(_, _)));
        assert!(matches!(body[2], Instruction::Load(_, _)));
        assert!(matches!(body[3], Instruction::Label(_)));
    }

    #[test]
    fn test_memory_ops() {
        let src = r#"
.func memtest(int r0) -> int
    ptr r1
    int r2
    alloc r1, r0
    store r1, r0
    read r2, r1
    free r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let body = &prog.funcs[0].body;
        assert!(matches!(body[0], Instruction::Alloc(_, _)));
        assert!(matches!(body[1], Instruction::Store(_, _)));
        assert!(matches!(body[2], Instruction::Read(_, _)));
        assert!(matches!(body[3], Instruction::Free(_)));
    }

    #[test]
    fn test_float_and_char() {
        let src = r#"
.func types() -> float
    float r0
    char r1
    load r0, 3.14
    load r1, 'Z'
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let body = &prog.funcs[0].body;
        match &body[0] {
            Instruction::Load(_, Operand::ImmFloat(f)) => assert!((*f - 3.14).abs() < 1e-10),
            _ => panic!("expected float load"),
        }
        match &body[1] {
            Instruction::Load(_, Operand::ImmChar(c)) => assert_eq!(*c, 'Z'),
            _ => panic!("expected char load"),
        }
    }

    #[test]
    fn test_negative_number() {
        let src = r#"
.func neg() -> int
    int r0
    load r0, -42
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        match &prog.funcs[0].body[0] {
            Instruction::Load(_, Operand::ImmInt(n)) => assert_eq!(*n, -42),
            _ => panic!("expected int load"),
        }
    }

    #[test]
    fn test_data_ref() {
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
        match &prog.funcs[0].body[0] {
            Instruction::Load(_, Operand::DataRef(name)) => assert_eq!(name, "msg"),
            _ => panic!("expected data ref"),
        }
    }

    #[test]
    fn test_void_func_bare_ret() {
        let src = r#"
.func nothing() -> void
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        assert!(matches!(prog.funcs[0].body[0], Instruction::Ret(None)));
    }

    #[test]
    fn test_call_variants() {
        let src = r#"
.extern puts(ptr) -> int

.section code
.func caller(ptr r0) -> int
    int r1
    call r1, caller, r0
    call.ext puts, r0
    call.void caller, r0
    call.ext.void puts, r0
    ret r1
.endfunc
"#;
        let prog = parse(src).unwrap();
        let body = &prog.funcs[0].body;
        assert!(matches!(body[0], Instruction::Call(_, _, _)));
        assert!(matches!(body[1], Instruction::CallExt(_, _)));
        assert!(matches!(body[2], Instruction::CallVoid(_, _)));
        assert!(matches!(body[3], Instruction::CallExtVoid(_, _)));
    }

    #[test]
    fn test_duplicate_func_error() {
        let src = r#"
.func foo() -> void
    ret
.endfunc
.func foo() -> void
    ret
.endfunc
"#;
        assert!(parse(src).is_err());
    }

    #[test]
    fn test_unknown_instruction_error() {
        let src = r#"
.func bad() -> void
    foobar r0, r1
    ret
.endfunc
"#;
        assert!(matches!(parse(src).unwrap_err(), AsmError::UnknownInstruction(_, _)));
    }

    #[test]
    fn test_unknown_type_error() {
        let src = r#"
.func bad(blob r0) -> void
    ret
.endfunc
"#;
        assert!(parse(src).is_err());
    }

    #[test]
    fn test_multiple_funcs() {
        let src = r#"
.func a() -> int
    int r0
    load r0, 1
    ret r0
.endfunc

.func b() -> int
    int r0
    load r0, 2
    ret r0
.endfunc

.func c() -> int
    int r0
    int r1
    call r0, a
    call r1, b
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        assert_eq!(prog.funcs.len(), 3);
    }

    #[test]
    fn test_checker_type_mismatch() {
        let src = r#"
.func bad(int r0, float r1) -> int
    int r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_checker_wrong_arg_count() {
        let src = r#"
.func foo(int r0) -> int
    ret r0
.endfunc

.func bar() -> int
    int r0
    call r0, foo
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_checker_ret_type_mismatch() {
        let src = r#"
.func bad() -> int
    float r0
    load r0, 1.0
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_resolver_undefined_label() {
        let src = r#"
.func bad() -> void
    jmp nowhere
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_resolver_undefined_func() {
        let src = r#"
.func bad() -> int
    int r0
    call r0, ghost
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_lint_unused_reg() {
        let src = r#"
.func bad() -> int
    int r0
    int r1
    load r0, 42
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (_, warnings) = analyze(&prog);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_lint_unreachable() {
        let src = r#"
.func bad() -> int
    int r0
    load r0, 1
    ret r0
    load r0, 2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (_, warnings) = analyze(&prog);
        assert!(!warnings.is_empty());
    }

    #[test]
    fn test_clean_program_no_errors() {
        let src = r#"
.func add(int r0, int r1) -> int
    int r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

}

#[cfg(test)]
mod audit_tests {
    use super::*;
    use crate::ast::*;
    use crate::error::AsmError;

    // --- resolver ---

    #[test]
    fn test_resolver_undefined_extern() {
        let src = r#"
.func bad() -> void
    call.ext ghost
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "expected undefined extern error");
        assert!(errors[0].message.contains("ghost"));
    }

    #[test]
    fn test_resolver_undefined_dataref() {
        let src = r#"
.func bad() -> ptr
    ptr r0
    load r0, @missing
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "expected undefined data ref error");
        assert!(errors[0].message.contains("missing"));
    }

    #[test]
    fn test_resolver_jmpif_undefined_label() {
        let src = r#"
.func bad(int r0) -> void
    jmpif r0, nowhere
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("nowhere"));
    }

    #[test]
    fn test_resolver_jmpifnot_undefined_label() {
        let src = r#"
.func bad(int r0) -> void
    jmpifnot r0, nowhere
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_resolver_valid_label_passes() {
        let src = r#"
.func ok(int r0) -> void
    jmpif r0, done
    jmp done
done
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "expected no errors: {:?}", errors);
    }

    #[test]
    fn test_resolver_call_void_undefined() {
        let src = r#"
.func bad() -> void
    call.void ghost
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
        assert!(errors[0].message.contains("ghost"));
    }

    #[test]
    fn test_resolver_call_ext_void_undefined() {
        let src = r#"
.func bad() -> void
    call.ext.void ghost
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty());
    }

    #[test]
    fn test_resolver_valid_dataref_passes() {
        let src = r#"
.section data
    .str msg "hello"

.section code
.func ok() -> ptr
    ptr r0
    load r0, @msg
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    }

    // --- checker ---

    #[test]
    fn test_checker_add_char_error() {
        let src = r#"
.func bad(char r0, char r1) -> char
    char r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "char arithmetic should fail");
    }

    #[test]
    fn test_checker_add_ptr_error() {
        let src = r#"
.func bad(ptr r0, ptr r1) -> ptr
    ptr r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "ptr arithmetic should fail");
    }

    #[test]
    fn test_checker_float_arithmetic_passes() {
        let src = r#"
.func ok(float r0, float r1) -> float
    float r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "float arithmetic should pass: {:?}", errors);
    }

    #[test]
    fn test_checker_dst_type_mismatch_arithmetic() {
        let src = r#"
.func bad(int r0, int r1) -> float
    float r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "dst type mismatch should fail");
    }

    #[test]
    fn test_checker_comparison_wrong_dst_type() {
        let src = r#"
.func bad(int r0, int r1) -> float
    float r2
    eq r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "comparison dst must be int");
    }

    #[test]
    fn test_checker_void_ret_in_void_func_passes() {
        let src = r#"
.func ok() -> void
    ret
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "bare ret in void func should pass: {:?}", errors);
    }

    #[test]
    fn test_checker_extern_arg_type_mismatch() {
        let src = r#"
.extern putchar(char) -> int

.section code
.func bad(int r0) -> int
    int r1
    call.ext putchar, r0
    ret r1
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "int passed to char param should fail");
    }

    #[test]
    fn test_checker_extern_arg_count_mismatch() {
        let src = r#"
.extern putchar(char) -> int

.section code
.func bad() -> int
    int r0
    call.ext putchar
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "wrong extern arg count should fail");
    }

    #[test]
    fn test_checker_load_type_mismatch_float_into_int() {
        let src = r#"
.func bad() -> int
    int r0
    load r0, 3.14
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "loading float into int register should fail");
    }

    #[test]
    fn test_checker_load_char_into_char_passes() {
        let src = r#"
.func ok() -> char
    char r0
    load r0, 'A'
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "loading char into char should pass: {:?}", errors);
    }

    // --- lint ---

    #[test]
    fn test_lint_unreachable_after_jmp() {
        let src = r#"
.func bad() -> int
    int r0
    load r0, 1
    jmp end
    load r0, 2
end
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (_, warnings) = analyze(&prog);
        assert!(!warnings.is_empty(), "expected unreachable warning after jmp");
    }

    #[test]
    fn test_lint_reachable_after_label_passes() {
        let src = r#"
.func ok(int r0) -> int
    int r1
    load r1, 0
    jmpif r0, skip
    load r1, 1
skip
    ret r1
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (_, warnings) = analyze(&prog);
        let unreachable: Vec<_> = warnings.iter()
            .filter(|w| w.message.contains("unreachable"))
            .collect();
        assert!(unreachable.is_empty(), "no unreachable after label: {:?}", unreachable);
    }

    #[test]
    fn test_lint_no_warnings_clean_func() {
        let src = r#"
.func ok(int r0, int r1) -> int
    int r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (_, warnings) = analyze(&prog);
        assert!(warnings.is_empty(), "clean func should have no warnings: {:?}", warnings);
    }

    #[test]
    fn test_lint_param_not_warned_as_unused() {
        // params are used even if only passed through - lint should not warn on them
        let src = r#"
.func ok(int r0) -> int
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (_, warnings) = analyze(&prog);
        assert!(warnings.is_empty(), "params should not be warned as unused: {:?}", warnings);
    }

    #[test]
    fn test_lint_multiple_unused_regs() {
        let src = r#"
.func bad() -> int
    int r0
    int r1
    int r2
    load r0, 1
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (_, warnings) = analyze(&prog);
        let unused: Vec<_> = warnings.iter()
            .filter(|w| w.message.contains("never read"))
            .collect();
        assert_eq!(unused.len(), 2, "expected 2 unused reg warnings, got {}", unused.len());
    }

    // --- delta-format error cases ---

    #[test]
    fn test_parse_unknown_section_tag_skipped() {
        use delta_format::file::DcFile;
        let mut bytes = Vec::new();
        // magic + version + section_count=1
        bytes.extend_from_slice(b"DC\x00\x01");
        bytes.extend_from_slice(&1u16.to_le_bytes()); // version
        bytes.extend_from_slice(&1u16.to_le_bytes()); // 1 section
        // unknown tag 0xFF, len=3, data=aaa
        bytes.push(0xFF);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"aaa");

        let dc = DcFile::deserialize(&bytes);
        assert!(dc.is_some(), "unknown section should be skipped, not fail");
        let dc = dc.unwrap();
        assert_eq!(dc.funcs.len(), 0);
        assert_eq!(dc.code.len(), 0);
    }

    #[test]
    fn test_parse_bad_magic_fails() {
        use delta_format::file::DcFile;
        let bytes = b"XXXX\x01\x00\x00\x00";
        assert!(DcFile::deserialize(bytes).is_none(), "bad magic should return None");
    }

    #[test]
    fn test_parse_truncated_data_fails() {
        use delta_format::file::DcFile;
        // valid magic but only 4 bytes total - truncated
        let bytes = b"DC\x00\x01";
        assert!(DcFile::deserialize(bytes).is_none(), "truncated file should return None");
    }

    #[test]
    fn test_format_c_no_args_roundtrip() {
        use delta_format::{encoding::Instr, opcode::Op};
        let instr = Instr::C { op: Op::CallVoid, dst: 0, func_idx: 0, args: vec![] };
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        // base 8 bytes + 0 args padded to 4 = 8
        assert_eq!(buf.len(), 8);
        let (decoded, pos) = Instr::decode(&buf, 0).unwrap();
        assert_eq!(pos, 8);
        assert_eq!(decoded, instr);
    }

    #[test]
    fn test_format_c_three_args_roundtrip() {
        use delta_format::{encoding::Instr, opcode::Op};
        // 3 args - padded to 4 bytes = 1 pad byte
        let instr = Instr::C { op: Op::Call, dst: 1, func_idx: 5, args: vec![0, 1, 2] };
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        // 8 base + 3 args + 1 pad = 12
        assert_eq!(buf.len(), 12);
        let (decoded, pos) = Instr::decode(&buf, 0).unwrap();
        assert_eq!(pos, 12);
        assert_eq!(decoded, instr);
    }

    #[test]
    fn test_format_c_four_args_roundtrip() {
        use delta_format::{encoding::Instr, opcode::Op};
        // 4 args - already aligned, no padding
        let instr = Instr::C { op: Op::Call, dst: 0, func_idx: 1, args: vec![0, 1, 2, 3] };
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        // 8 base + 4 args + 0 pad = 12
        assert_eq!(buf.len(), 12);
        let (decoded, pos) = Instr::decode(&buf, 0).unwrap();
        assert_eq!(pos, 12);
        assert_eq!(decoded, instr);
    }

    #[test]
    fn test_dc_data_entries_roundtrip() {
        use delta_format::file::{DcFile, DataEntry};
        let mut dc = DcFile::default();
        dc.data.push(DataEntry::Str(b"hello\0".to_vec()));
        dc.data.push(DataEntry::Int(-999));
        dc.data.push(DataEntry::Float(-3.14));

        let bytes = dc.serialize();
        let loaded = DcFile::deserialize(&bytes).unwrap();
        assert_eq!(loaded.data.len(), 3);

        match &loaded.data[0] {
            DataEntry::Str(s) => assert_eq!(s, b"hello\0"),
            _ => panic!("expected Str"),
        }
        match &loaded.data[1] {
            DataEntry::Int(n) => assert_eq!(*n, -999),
            _ => panic!("expected Int"),
        }
        match &loaded.data[2] {
            DataEntry::Float(f) => assert!((*f - (-3.14f64)).abs() < 1e-10),
            _ => panic!("expected Float"),
        }
    }

    #[test]
    fn test_opcode_from_u8_unknown_returns_none() {
        use delta_format::opcode::Op;
        assert!(Op::from_u8(0x00).is_none());
        assert!(Op::from_u8(0xFF).is_none());
        assert!(Op::from_u8(0xF0).is_none()); // 0xD0-0xD5 bitwise, 0xE0-0xE3 func pointers/panic
    }

    #[test]
    fn test_opcode_roundtrip_all_known() {
        use delta_format::opcode::Op;
        let known: &[u8] = &[
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08,
            0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15,
            0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D,
            0x20, 0x21, 0x22, 0x23,
            0x30, 0x31, 0x32, 0x33, 0x34,
            0x40, 0x41, 0x42,
            0x50, 0x51, 0x52, 0x53,
            0x60, 0x61,
            0x70, 0x71, 0x72, 0x73,
            0x80, 0x81, 0x82,
            0x8E, 0x8F,
            0x90, 0x91, 0x92, 0x93, 0x94,
            0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0xA6,
        ];
        for &byte in known {
            assert!(Op::from_u8(byte).is_some(), "opcode 0x{byte:02X} should be known");
            let op = Op::from_u8(byte).unwrap();
            assert_eq!(op as u8, byte, "opcode 0x{byte:02X} roundtrip failed");
        }
    }
}

#[cfg(test)]
mod bool_tests {
    use super::*;
    use crate::ast::*;

    #[test]
    fn test_bool_parses_as_type() {
        let src = r#"
.func ok() -> bool
    bool r0
    load r0, 1
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        assert_eq!(prog.funcs[0].ret_type, Type::Bool);
        assert_eq!(prog.funcs[0].locals[0].ty, Type::Bool);
    }

    #[test]
    fn test_bool_param_parses() {
        let src = r#"
.func ok(bool r0) -> bool
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        assert_eq!(prog.funcs[0].params[0].ty, Type::Bool);
    }

    #[test]
    fn test_bool_no_checker_errors() {
        let src = r#"
.func ok() -> bool
    bool r0
    load r0, 1
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "bool func should pass checker: {:?}", errors);
    }

    #[test]
    fn test_bool_compat_with_int_param() {
        // passing bool to int param and vice versa should pass
        let src = r#"
.func takes_int(int r0) -> int
    ret r0
.endfunc

.func caller() -> int
    bool b
    int r
    load b, 1
    call r, takes_int, b
    ret r
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "bool->int compat should pass: {:?}", errors);
    }

    #[test]
    fn test_int_compat_with_bool_param() {
        let src = r#"
.func takes_bool(bool r0) -> bool
    ret r0
.endfunc

.func caller() -> bool
    int n
    bool r
    load n, 0
    call r, takes_bool, n
    ret r
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "int->bool compat should pass: {:?}", errors);
    }

    #[test]
    fn test_bool_ret_compat_with_int_ret() {
        // function declared -> bool, returning int register
        let src = r#"
.func ok() -> bool
    int r0
    load r0, 0
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "int ret in bool func should pass: {:?}", errors);
    }

    #[test]
    fn test_bool_comparison_dst() {
        // comparison result into bool register should pass
        let src = r#"
.func ok(int r0) -> bool
    bool result
    gt result, r0, 0
    ret result
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "comparison into bool should pass: {:?}", errors);
    }

    #[test]
    fn test_bool_arithmetic() {
        // bool + bool should work like int + int
        let src = r#"
.func ok(bool r0, bool r1) -> bool
    bool r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "bool arithmetic should pass: {:?}", errors);
    }

    #[test]
    fn test_bool_bitwise() {
        let src = r#"
.func ok(bool r0, bool r1) -> bool
    bool r2
    and r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "bool bitwise should pass: {:?}", errors);
    }

    #[test]
    fn test_bool_float_mismatch_fails() {
        // bool and float should still be incompatible
        let src = r#"
.func bad(bool r0, float r1) -> bool
    bool r2
    add r2, r0, r1
    ret r2
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(!errors.is_empty(), "bool+float should fail");
    }

    #[test]
    fn test_bool_load_int_literal() {
        // loading an int literal (0 or 1) into a bool register should pass
        let src = r#"
.func ok() -> bool
    bool r0
    load r0, 0
    ret r0
.endfunc
"#;
        let prog = parse(src).unwrap();
        let (errors, _) = analyze(&prog);
        assert!(errors.is_empty(), "loading int literal into bool should pass: {:?}", errors);
    }
}

