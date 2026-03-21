// abstract syntax tree for .ds source files

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    Bool,
    Float,
    Char,
    Ptr,
    Void,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub ty: Type,
    pub name: String,
}

#[derive(Debug, Clone)]
pub enum Operand {
    Reg(String),
    ImmInt(i64),
    ImmFloat(f64),
    ImmChar(char),
    DataRef(String),
}

#[derive(Debug, Clone)]
pub enum Instruction {
    // arithmetic: dst, src_a, src_b
    Add(String, Operand, Operand),
    Sub(String, Operand, Operand),
    Mul(String, Operand, Operand),
    Div(String, Operand, Operand),

    // load immediate or data ref into register
    Load(String, Operand),

    // memory: manual alloc/free
    Alloc(String, Operand),
    Free(Operand),
    Store(Operand, Operand),
    Read(String, Operand),

    // control flow
    Jmp(String),
    JmpIf(Operand, String),
    JmpIfNot(Operand, String),

    // comparisons: dst, a, b
    Eq(String, Operand, Operand),
    Ne(String, Operand, Operand),
    Lt(String, Operand, Operand),
    Le(String, Operand, Operand),
    Gt(String, Operand, Operand),
    Ge(String, Operand, Operand),

    // call dst_reg, func_name, args
    Call(String, String, Vec<Operand>),
    // call.ext func_name, args (return stored externally or discarded)
    CallExt(String, Vec<Operand>),
    // call.void - discards return value
    CallVoid(String, Vec<Operand>),
    CallExtVoid(String, Vec<Operand>),

    Ret(Option<Operand>),

    // label definition inside a func body
    Label(String),

    // print value of a register to stdout
    Print(Operand),
    PrintInt(Operand),
    PrintFloat(Operand),
    PrintChar(Operand),
    PrintPtr(Operand),

    // time - store result into dst register (int)
    TimeNs(String),      // unix timestamp nanoseconds
    TimeMs(String),      // unix timestamp milliseconds
    TimeMonoNs(String),  // monotonic nanoseconds since VM start

    // extended arithmetic - binary (dst, a, b)
    ModInt(String, Operand, Operand),
    ModFloat(String, Operand, Operand),
    PowInt(String, Operand, Operand),
    PowFloat(String, Operand, Operand),

    // extended arithmetic - unary (dst, src)
    NegInt(String, Operand),
    NegFloat(String, Operand),
    AbsInt(String, Operand),
    AbsFloat(String, Operand),
    SqrtFloat(String, Operand),

    // type casts (dst, src)
    IntToFloat(String, Operand),
    FloatToInt(String, Operand),
    IntToChar(String, Operand),
    CharToInt(String, Operand),
    PtrToInt(String, Operand),

    // string / char ops
    StrLen(String, Operand),             // dst:int  = strlen(src:ptr)
    StrEq(String, Operand, Operand),     // dst:int  = (a:ptr == b:ptr)
    StrCharAt(String, Operand, Operand), // dst:char = str[idx:int]
    CharToUpper(String, Operand),        // dst:char = toupper(src:char)
    CharToLower(String, Operand),        // dst:char = tolower(src:char)
    IntToStr(String, Operand),           // dst:ptr  = itoa(src:int)
    FloatToStr(String, Operand),         // dst:ptr  = ftoa(src:float)

    // arrays
    ArrNew(String, Operand),             // dst:ptr = arr_new(size: imm or reg)
    ArrGet(String, Operand, Operand),    // dst = arr[idx]
    ArrSet(Operand, Operand, Operand),   // arr[idx] = val  (no dst)
    ArrLen(String, Operand),             // dst:int = arr_len(arr)
    ArrFree(Operand),                    // free array

    // stdin input
    ReadChar(String),                    // dst:char
    ReadInt(String),                     // dst:int
    ReadFloat(String),                   // dst:float
    ReadLine(String),                    // dst:ptr  (heap-allocated, newline stripped)

    // bitwise (int only)
    BitAnd(String, Operand, Operand),    // dst = a & b
    BitOr(String, Operand, Operand),     // dst = a | b
    BitXor(String, Operand, Operand),    // dst = a ^ b
    BitNot(String, Operand),             // dst = ~a
    Shl(String, Operand, Operand),       // dst = a << b
    Shr(String, Operand, Operand),       // dst = a >> b  (arithmetic)

    // function pointers
    FuncPtr(String, String),             // dst:ptr = address of func_name
    CallPtr(String, Operand, Vec<Operand>),      // dst = (*fptr)(args)
    CallPtrVoid(Operand, Vec<Operand>),          // (*fptr)(args)

    // panic
    Panic(Operand),                      // print message and exit(1)
}

#[derive(Debug, Clone)]
pub struct Register {
    pub name: String,
    pub ty: Type,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub ret_type: Type,
    pub locals: Vec<Register>,
    pub body: Vec<Instruction>,
}

#[derive(Debug, Clone)]
pub struct ExternDecl {
    pub name: String,
    pub params: Vec<Type>,
    pub ret_type: Type,
    pub variadic: bool,   // true if declared with ... at end of param list
}

#[derive(Debug, Clone)]
pub enum DataItem {
    Str(String, String),
    Int(String, i64),
    Float(String, f64),
}

#[derive(Debug, Default)]
pub struct Program {
    pub externs: Vec<ExternDecl>,
    pub data: Vec<DataItem>,
    pub funcs: Vec<FuncDecl>,
}
