// all opcodes for the delta VM instruction set
// values are stable - adding new ones goes at the end of each group

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    // arithmetic reg-reg (format A)
    AddInt = 0x01,
    SubInt = 0x02,
    MulInt = 0x03,
    DivInt = 0x04,
    AddFloat = 0x05,
    SubFloat = 0x06,
    MulFloat = 0x07,
    DivFloat = 0x08,

    // extended arithmetic (format A for binary, format D for unary)
    ModInt = 0x09,     // format A: dst = a % b
    ModFloat = 0x0A,   // format A: dst = fmod(a, b)
    PowInt = 0x0B,     // format A: dst = a ^ b (integer power)
    PowFloat = 0x0C,   // format A: dst = a ^ b
    NegInt = 0x0D,     // format D: dst = -src
    NegFloat = 0x0E,   // format D: dst = -src
    AbsInt = 0x0F,     // format D: dst = |src|
    AbsFloat = 0x8F,   // format D: dst = |src|
    SqrtFloat = 0x8E,  // format D: dst = sqrt(src)

    // comparisons reg-reg -> int dst (format A)
    EqInt = 0x10,
    NeInt = 0x11,
    LtInt = 0x12,
    LeInt = 0x13,
    GtInt = 0x14,
    GeInt = 0x15,
    EqFloat = 0x16,
    NeFloat = 0x17,
    LtFloat = 0x18,
    LeFloat = 0x19,
    GtFloat = 0x1A,
    GeFloat = 0x1B,
    EqChar = 0x1C,
    NeChar = 0x1D,

    // load immediate into register (format B)
    LoadInt = 0x20,
    LoadFloat = 0x21,
    LoadChar = 0x22,
    LoadPtr = 0x23,   // load data-section address by index

    // memory (format A / format B)
    Alloc = 0x30,     // format B: dst, size_imm
    AllocReg = 0x31,  // format A: dst, size_reg
    Free = 0x32,      // format D: src
    Store = 0x33,     // format A: ptr, val (no dst)
    Read = 0x34,      // format A: dst, ptr

    // control flow (format B: imm = target byte offset)
    Jmp = 0x40,
    JmpIf = 0x41,     // format B: cond_reg in dst field, imm = offset
    JmpIfNot = 0x42,

    // calls (format C)
    Call = 0x50,
    CallVoid = 0x51,
    CallExt = 0x52,
    CallExtVoid = 0x53,

    // return (format D)
    Ret = 0x60,
    RetVoid = 0x61,

    // print (format D: src reg)
    PrintInt = 0x70,
    PrintFloat = 0x71,
    PrintChar = 0x72,
    PrintPtr = 0x73,

    // time (format D: dst reg)
    TimeNs = 0x80,     // unix timestamp in nanoseconds -> int
    TimeMs = 0x81,     // unix timestamp in milliseconds -> int
    TimeMonoNs = 0x82, // monotonic nanoseconds since VM start -> int

    // type casts (format D: dst = cast(src))
    IntToFloat = 0x90,
    FloatToInt = 0x91,
    IntToChar = 0x92,
    CharToInt = 0x93,
    PtrToInt = 0x94,

    // string / char ops (format A or format D)
    StrLen = 0xA0,      // format D: dst:int = strlen(src:ptr)
    StrEq = 0xA1,       // format A: dst:int = (a:ptr == b:ptr)
    StrCharAt = 0xA2,   // format A: dst:char = str[index:int]
    CharToUpper = 0xA3, // format D: dst:char = toupper(src:char)
    CharToLower = 0xA4, // format D: dst:char = tolower(src:char)
    IntToStr = 0xA5,    // format D: dst:ptr = itoa(src:int)  - allocates heap string
    FloatToStr = 0xA6,  // format D: dst:ptr = ftoa(src:float) - allocates heap string

    // input (format D: dst reg)
    ReadChar = 0xB0,
    ReadInt = 0xB1,
    ReadFloat = 0xB2,
    ReadLine = 0xB3,

    // arrays: layout [len:i64][e0..eN-1:i64]
    ArrNew = 0xC0,     // format B: dst = arr_new(size:imm)
    ArrNewReg = 0xC1,  // format A: dst = arr_new(size:reg)
    ArrGet = 0xC2,     // format A: dst = arr[a_ptr][b_idx]
    ArrSet = 0xC3,     // format A: arr[dst_ptr][a_idx] = b_val
    ArrLen = 0xC4,     // format A: dst = arr_len(a_ptr)
    ArrFree = 0xC5,    // format D: free array

    // bitwise integer ops (format A for binary, format D-style A for unary)
    BitAnd = 0xD0,     // format A: dst = a & b
    BitOr  = 0xD1,     // format A: dst = a | b
    BitXor = 0xD2,     // format A: dst = a ^ b
    BitNot = 0xD3,     // format A: dst = ~a  (b unused)
    Shl    = 0xD4,     // format A: dst = a << b
    Shr    = 0xD5,     // format A: dst = a >> b  (arithmetic right shift)

    // function pointers
    FuncPtr    = 0xE0, // format B: dst:ptr = address of func[imm]
    CallPtr    = 0xE1, // format C: dst = (*fptr)(args)  - fptr in func_idx field as reg
    CallPtrVoid= 0xE2, // format C: (*fptr)(args)  - void

    // control
    Panic = 0xE3,      // format D: print string at src, exit(1)
}

impl Op {
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0x01 => Some(Op::AddInt),
            0x02 => Some(Op::SubInt),
            0x03 => Some(Op::MulInt),
            0x04 => Some(Op::DivInt),
            0x05 => Some(Op::AddFloat),
            0x06 => Some(Op::SubFloat),
            0x07 => Some(Op::MulFloat),
            0x08 => Some(Op::DivFloat),
            0x09 => Some(Op::ModInt),
            0x0A => Some(Op::ModFloat),
            0x0B => Some(Op::PowInt),
            0x0C => Some(Op::PowFloat),
            0x0D => Some(Op::NegInt),
            0x0E => Some(Op::NegFloat),
            0x0F => Some(Op::AbsInt),
            0x8E => Some(Op::SqrtFloat),
            0x8F => Some(Op::AbsFloat),
            0x10 => Some(Op::EqInt),
            0x11 => Some(Op::NeInt),
            0x12 => Some(Op::LtInt),
            0x13 => Some(Op::LeInt),
            0x14 => Some(Op::GtInt),
            0x15 => Some(Op::GeInt),
            0x16 => Some(Op::EqFloat),
            0x17 => Some(Op::NeFloat),
            0x18 => Some(Op::LtFloat),
            0x19 => Some(Op::LeFloat),
            0x1A => Some(Op::GtFloat),
            0x1B => Some(Op::GeFloat),
            0x1C => Some(Op::EqChar),
            0x1D => Some(Op::NeChar),
            0x20 => Some(Op::LoadInt),
            0x21 => Some(Op::LoadFloat),
            0x22 => Some(Op::LoadChar),
            0x23 => Some(Op::LoadPtr),
            0x30 => Some(Op::Alloc),
            0x31 => Some(Op::AllocReg),
            0x32 => Some(Op::Free),
            0x33 => Some(Op::Store),
            0x34 => Some(Op::Read),
            0x40 => Some(Op::Jmp),
            0x41 => Some(Op::JmpIf),
            0x42 => Some(Op::JmpIfNot),
            0x50 => Some(Op::Call),
            0x51 => Some(Op::CallVoid),
            0x52 => Some(Op::CallExt),
            0x53 => Some(Op::CallExtVoid),
            0x60 => Some(Op::Ret),
            0x61 => Some(Op::RetVoid),
            0x70 => Some(Op::PrintInt),
            0x71 => Some(Op::PrintFloat),
            0x72 => Some(Op::PrintChar),
            0x73 => Some(Op::PrintPtr),
            0x80 => Some(Op::TimeNs),
            0x81 => Some(Op::TimeMs),
            0x82 => Some(Op::TimeMonoNs),
            0x90 => Some(Op::IntToFloat),
            0x91 => Some(Op::FloatToInt),
            0x92 => Some(Op::IntToChar),
            0x93 => Some(Op::CharToInt),
            0x94 => Some(Op::PtrToInt),
            0xA0 => Some(Op::StrLen),
            0xA1 => Some(Op::StrEq),
            0xA2 => Some(Op::StrCharAt),
            0xA3 => Some(Op::CharToUpper),
            0xA4 => Some(Op::CharToLower),
            0xA5 => Some(Op::IntToStr),
            0xA6 => Some(Op::FloatToStr),
            0xB0 => Some(Op::ReadChar),
            0xB1 => Some(Op::ReadInt),
            0xB2 => Some(Op::ReadFloat),
            0xB3 => Some(Op::ReadLine),
            0xC0 => Some(Op::ArrNew),
            0xC1 => Some(Op::ArrNewReg),
            0xC2 => Some(Op::ArrGet),
            0xC3 => Some(Op::ArrSet),
            0xC4 => Some(Op::ArrLen),
            0xC5 => Some(Op::ArrFree),
            0xD0 => Some(Op::BitAnd),
            0xD1 => Some(Op::BitOr),
            0xD2 => Some(Op::BitXor),
            0xD3 => Some(Op::BitNot),
            0xD4 => Some(Op::Shl),
            0xD5 => Some(Op::Shr),
            0xE0 => Some(Op::FuncPtr),
            0xE1 => Some(Op::CallPtr),
            0xE2 => Some(Op::CallPtrVoid),
            0xE3 => Some(Op::Panic),
            _ => None,
        }
    }
}
