// instruction encoding
//
// format A - reg-reg (4 bytes):   [op:8][dst:8][a:8][b:8]
// format B - reg-imm (8 bytes):   [op:8][dst:8][pad:16][imm:32]
// format C - call (variable):     [op:8][dst:8][func:16][argc:8][pad:24] [arg0:8][arg1:8]...
//                                  base = 8 bytes + argc bytes, padded to 4-byte boundary
// format D - single reg (4 bytes): [op:8][src:8][pad:16]

use crate::opcode::Op;

#[derive(Debug, Clone, PartialEq)]
pub enum Instr {
    // format A
    A { op: Op, dst: u8, a: u8, b: u8 },
    // format B
    B { op: Op, dst: u8, imm: u32 },
    // format C
    C { op: Op, dst: u8, func_idx: u16, args: Vec<u8> },
    // format D
    D { op: Op, src: u8 },
}

impl Instr {
    // encode instruction to bytes, appending into buf
    pub fn encode(&self, buf: &mut Vec<u8>) {
        match self {
            Instr::A { op, dst, a, b } => {
                buf.push(*op as u8);
                buf.push(*dst);
                buf.push(*a);
                buf.push(*b);
            }
            Instr::B { op, dst, imm } => {
                buf.push(*op as u8);
                buf.push(*dst);
                buf.push(0x00); // pad
                buf.push(0x00); // pad
                buf.extend_from_slice(&imm.to_le_bytes());
            }
            Instr::C { op, dst, func_idx, args } => {
                buf.push(*op as u8);
                buf.push(*dst);
                buf.extend_from_slice(&func_idx.to_le_bytes());
                buf.push(args.len() as u8);
                buf.push(0x00); // pad
                buf.push(0x00); // pad
                buf.push(0x00); // pad
                buf.extend_from_slice(args);
                // pad args to 4-byte boundary
                let args_padded = (args.len() + 3) & !3;
                for _ in args.len()..args_padded {
                    buf.push(0x00);
                }
            }
            Instr::D { op, src } => {
                buf.push(*op as u8);
                buf.push(*src);
                buf.push(0x00); // pad
                buf.push(0x00); // pad
            }
        }
    }

    // decode one instruction from buf at pos, returns (instr, new_pos)
    pub fn decode(buf: &[u8], pos: usize) -> Option<(Instr, usize)> {
        let op_byte = *buf.get(pos)?;
        let op = Op::from_u8(op_byte)?;

        match op {
            // format D - single reg
            Op::Free | Op::Ret | Op::RetVoid |
            Op::PrintInt | Op::PrintFloat | Op::PrintChar | Op::PrintPtr |
            Op::TimeNs | Op::TimeMs | Op::TimeMonoNs |
            Op::ReadChar | Op::ReadInt | Op::ReadFloat | Op::ReadLine |
            Op::ArrFree | Op::Panic => {                let src = *buf.get(pos + 1)?;
                Some((Instr::D { op, src }, pos + 4))
            }

            // format B - reg + immediate
            Op::LoadInt | Op::LoadFloat | Op::LoadChar | Op::LoadPtr |
            Op::Alloc | Op::Jmp | Op::JmpIf | Op::JmpIfNot | Op::ArrNew | Op::FuncPtr => {
                let dst = *buf.get(pos + 1)?;
                let imm = u32::from_le_bytes(buf.get(pos + 4..pos + 8)?.try_into().ok()?);
                Some((Instr::B { op, dst, imm }, pos + 8))
            }

            // format C - call
            Op::Call | Op::CallVoid | Op::CallExt | Op::CallExtVoid |
            Op::CallPtr | Op::CallPtrVoid => {
                let dst = *buf.get(pos + 1)?;
                let func_idx = u16::from_le_bytes(buf.get(pos + 2..pos + 4)?.try_into().ok()?);
                let argc = *buf.get(pos + 4)? as usize;
                let args_start = pos + 8;
                let args: Vec<u8> = buf.get(args_start..args_start + argc)?.to_vec();
                let args_padded = (argc + 3) & !3;
                Some((Instr::C { op, dst, func_idx, args }, args_start + args_padded))
            }

            // format A - reg-reg (everything else)
            _ => {
                let dst = *buf.get(pos + 1)?;
                let a = *buf.get(pos + 2)?;
                let b = *buf.get(pos + 3)?;
                Some((Instr::A { op, dst, a, b }, pos + 4))
            }
        }
    }
}

// encode f64 as u32 for format B (stored as raw bits, reinterpreted on load)
// delta uses f64 internally but constants fit in 32-bit float range for immediates
// full 64-bit floats go through the constant pool
pub fn f32_bits(v: f64) -> u32 {
    (v as f32).to_bits()
}

pub fn bits_to_f32(bits: u32) -> f64 {
    f32::from_bits(bits) as f64
}

// decodes a function's bytecode slice into a Vec<Instr>, converting jump targets
// from byte offsets (as stored in .dc) into instruction indices (for fast dispatch)
pub fn decode_func(code: &[u8], offset: usize, len: usize) -> Option<Vec<Instr>> {
    let end = offset + len;
    let mut instrs = Vec::new();
    // maps byte_offset_within_func -> instruction_index
    let mut byte_to_idx: Vec<(usize, usize)> = Vec::new();
    let mut pos = offset;

    // first pass: decode all instructions and record byte->index mapping
    while pos < end {
        let byte_offset = pos - offset;
        let (instr, next_pos) = Instr::decode(code, pos)?;
        byte_to_idx.push((byte_offset, instrs.len()));
        instrs.push(instr);
        pos = next_pos;
    }

    // second pass: rewrite jump imm values from byte offsets to instruction indices
    for instr in &mut instrs {
        match instr {
            Instr::B { op: Op::Jmp, imm, .. } |
            Instr::B { op: Op::JmpIf, imm, .. } |
            Instr::B { op: Op::JmpIfNot, imm, .. } => {
                let target_byte = *imm as usize;
                let idx = byte_to_idx.iter()
                    .find(|(b, _)| *b == target_byte)
                    .map(|(_, i)| *i)?;
                *imm = idx as u32;
            }
            _ => {}
        }
    }

    Some(instrs)
}
