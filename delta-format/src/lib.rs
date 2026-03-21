pub mod encoding;
pub mod file;
pub mod opcode;

pub use encoding::Instr;
pub use file::{DcFile, DataEntry, ExternEntry, FuncEntry};
pub use opcode::Op;

#[cfg(test)]
mod tests {
    use super::*;
    use encoding::Instr;
    use file::{DataEntry, DcFile, ExternEntry, FuncEntry};
    use opcode::Op;

    #[test]
    fn test_format_a_roundtrip() {
        let instr = Instr::A { op: Op::AddInt, dst: 2, a: 0, b: 1 };
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        assert_eq!(buf.len(), 4);
        assert_eq!(buf[0], Op::AddInt as u8);
        let (decoded, pos) = Instr::decode(&buf, 0).unwrap();
        assert_eq!(pos, 4);
        assert_eq!(decoded, instr);
    }

    #[test]
    fn test_format_b_roundtrip() {
        let instr = Instr::B { op: Op::LoadInt, dst: 0, imm: 42 };
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        assert_eq!(buf.len(), 8);
        let (decoded, pos) = Instr::decode(&buf, 0).unwrap();
        assert_eq!(pos, 8);
        assert_eq!(decoded, instr);
    }

    #[test]
    fn test_format_c_roundtrip() {
        let instr = Instr::C { op: Op::Call, dst: 0, func_idx: 3, args: vec![1, 2] };
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        assert_eq!(buf.len(), 12);
        let (decoded, pos) = Instr::decode(&buf, 0).unwrap();
        assert_eq!(pos, 12);
        assert_eq!(decoded, instr);
    }

    #[test]
    fn test_format_d_roundtrip() {
        let instr = Instr::D { op: Op::Ret, src: 1 };
        let mut buf = Vec::new();
        instr.encode(&mut buf);
        assert_eq!(buf.len(), 4);
        let (decoded, pos) = Instr::decode(&buf, 0).unwrap();
        assert_eq!(pos, 4);
        assert_eq!(decoded, instr);
    }

    #[test]
    fn test_dc_file_roundtrip() {
        let mut dc = DcFile::default();
        dc.funcs.push(FuncEntry {
            code_offset: 0,
            code_len: 12,
            reg_count: 3,
            param_count: 2,
            name: "add".into(),
        });
        let mut code = Vec::new();
        Instr::B { op: Op::LoadInt, dst: 2, imm: 0 }.encode(&mut code);
        Instr::D { op: Op::Ret, src: 2 }.encode(&mut code);
        dc.code = code;
        dc.data.push(DataEntry::Str(b"hello\0".to_vec()));
        dc.data.push(DataEntry::Int(42));
        dc.data.push(DataEntry::Float(3.14));
        dc.externs.push(ExternEntry { name: "putchar".into(), param_count: 1, variadic: false });

        let bytes = dc.serialize();
        assert_eq!(&bytes[0..4], b"DC\x00\x01");

        let loaded = DcFile::deserialize(&bytes).unwrap();
        assert_eq!(loaded.funcs.len(), 1);
        assert_eq!(loaded.funcs[0].name, "add");
        assert_eq!(loaded.funcs[0].reg_count, 3);
        assert_eq!(loaded.data.len(), 3);
        assert_eq!(loaded.externs.len(), 1);
        assert_eq!(loaded.externs[0].name, "putchar");
    }

    #[test]
    fn test_multiple_instructions_sequential() {
        let mut buf = Vec::new();
        Instr::B { op: Op::LoadInt, dst: 0, imm: 10 }.encode(&mut buf);
        Instr::B { op: Op::LoadInt, dst: 1, imm: 20 }.encode(&mut buf);
        Instr::A { op: Op::AddInt, dst: 2, a: 0, b: 1 }.encode(&mut buf);
        Instr::D { op: Op::Ret, src: 2 }.encode(&mut buf);

        let mut pos = 0;
        let mut count = 0;
        while pos < buf.len() {
            let (_, next) = Instr::decode(&buf, pos).unwrap();
            pos = next;
            count += 1;
        }
        assert_eq!(count, 4);
    }
}
