use crate::ast::*;
use crate::error::{AsmError, Result, Span};
use crate::lexer::Token;

pub struct Parser {
    tokens: Vec<(Token, Span)>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<(Token, Span)>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos].0
    }

    fn span(&self) -> Span {
        self.tokens[self.pos].1.clone()
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].0.clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn skip_newlines(&mut self) {
        while *self.peek() == Token::Newline {
            self.advance();
        }
    }

    fn expect(&mut self, expected: &Token) -> Result<()> {
        if self.peek() == expected {
            self.advance();
            Ok(())
        } else {
            Err(AsmError::UnexpectedToken(format!("{:?}", self.peek()), self.span()))
        }
    }

    fn expect_ident(&mut self) -> Result<String> {
        match self.advance() {
            Token::Ident(s) => Ok(s),
            t => Err(AsmError::UnexpectedToken(format!("{:?}", t), self.span())),
        }
    }

    fn parse_type(&mut self) -> Result<Type> {
        let span = self.span();
        match self.advance() {
            Token::TyInt => Ok(Type::Int),
            Token::TyBool => Ok(Type::Bool),
            Token::TyFloat => Ok(Type::Float),
            Token::TyChar => Ok(Type::Char),
            Token::TyPtr => Ok(Type::Ptr),
            Token::TyVoid => Ok(Type::Void),
            Token::Ident(s) => Err(AsmError::UnknownType(s, span)),
            t => Err(AsmError::UnexpectedToken(format!("{:?}", t), span)),
        }
    }

    fn parse_params(&mut self) -> Result<Vec<Param>> {
        self.expect(&Token::Lparen)?;
        let mut params = Vec::new();
        if *self.peek() == Token::Rparen {
            self.advance();
            return Ok(params);
        }
        loop {
            let ty = self.parse_type()?;
            let name = self.expect_ident()?;
            params.push(Param { ty, name });
            match self.peek() {
                Token::Comma  => { self.advance(); }
                Token::Rparen => { self.advance(); break; }
                _ => return Err(AsmError::UnexpectedToken(format!("{:?}", self.peek()), self.span())),
            }
        }
        Ok(params)
    }

    fn parse_extern_params(&mut self) -> Result<(Vec<Type>, bool)> {
        self.expect(&Token::Lparen)?;
        let mut types = Vec::new();
        let mut variadic = false;
        if *self.peek() == Token::Rparen {
            self.advance();
            return Ok((types, false));
        }
        loop {
            if *self.peek() == Token::Ellipsis {
                self.advance();
                variadic = true;
                // must be followed by )
                match self.peek() {
                    Token::Rparen => { self.advance(); break; }
                    _ => return Err(AsmError::UnexpectedToken(format!("{:?}", self.peek()), self.span())),
                }
            }
            types.push(self.parse_type()?);
            match self.peek() {
                Token::Comma  => { self.advance(); }
                Token::Rparen => { self.advance(); break; }
                _ => return Err(AsmError::UnexpectedToken(format!("{:?}", self.peek()), self.span())),
            }
        }
        Ok((types, variadic))
    }

    fn parse_operand(&mut self) -> Result<Operand> {
        let span = self.span();
        match self.advance() {
            Token::Ident(s)    => Ok(Operand::Reg(s)),
            Token::LitInt(n)   => Ok(Operand::ImmInt(n)),
            Token::LitFloat(f) => Ok(Operand::ImmFloat(f)),
            Token::LitChar(c)  => Ok(Operand::ImmChar(c)),
            Token::At => {
                let name = self.expect_ident()?;
                Ok(Operand::DataRef(name))
            }
            t => Err(AsmError::UnexpectedToken(format!("{:?}", t), span)),
        }
    }

    fn parse_arg_list(&mut self) -> Result<Vec<Operand>> {
        let mut args = Vec::new();
        // args are optional - stop at newline/eof
        match self.peek() {
            Token::Newline | Token::Eof => return Ok(args),
            _ => {}
        }
        args.push(self.parse_operand()?);
        while *self.peek() == Token::Comma {
            self.advance();
            args.push(self.parse_operand()?);
        }
        Ok(args)
    }

    fn parse_binary_op<F>(&mut self, constructor: F) -> Result<Instruction>
    where
        F: Fn(String, Operand, Operand) -> Instruction,
    {
        let dst = self.expect_ident()?;
        self.expect(&Token::Comma)?;
        let a = self.parse_operand()?;
        self.expect(&Token::Comma)?;
        let b = self.parse_operand()?;
        Ok(constructor(dst, a, b))
    }

    fn parse_unary_op<F>(&mut self, constructor: F) -> Result<Instruction>
    where
        F: Fn(String, Operand) -> Instruction,
    {
        let dst = self.expect_ident()?;
        self.expect(&Token::Comma)?;
        let src = self.parse_operand()?;
        Ok(constructor(dst, src))
    }

    fn parse_instruction(&mut self, mnemonic: String, span: Span) -> Result<Instruction> {
        match mnemonic.as_str() {
            "add" => self.parse_binary_op(Instruction::Add),
            "sub" => self.parse_binary_op(Instruction::Sub),
            "mul" => self.parse_binary_op(Instruction::Mul),
            "div" => self.parse_binary_op(Instruction::Div),
            "eq"  => self.parse_binary_op(Instruction::Eq),
            "ne"  => self.parse_binary_op(Instruction::Ne),
            "lt"  => self.parse_binary_op(Instruction::Lt),
            "le"  => self.parse_binary_op(Instruction::Le),
            "gt"  => self.parse_binary_op(Instruction::Gt),
            "ge"  => self.parse_binary_op(Instruction::Ge),

            "load" => {
                let dst = self.expect_ident()?;
                self.expect(&Token::Comma)?;
                let op = self.parse_operand()?;
                Ok(Instruction::Load(dst, op))
            }

            "alloc" => {
                let dst = self.expect_ident()?;
                self.expect(&Token::Comma)?;
                let size = self.parse_operand()?;
                Ok(Instruction::Alloc(dst, size))
            }

            "free" => {
                let op = self.parse_operand()?;
                Ok(Instruction::Free(op))
            }

            "store" => {
                let ptr = self.parse_operand()?;
                self.expect(&Token::Comma)?;
                let val = self.parse_operand()?;
                Ok(Instruction::Store(ptr, val))
            }

            "read" => {
                let dst = self.expect_ident()?;
                self.expect(&Token::Comma)?;
                let ptr = self.parse_operand()?;
                Ok(Instruction::Read(dst, ptr))
            }

            "jmp" => {
                let label = self.expect_ident()?;
                Ok(Instruction::Jmp(label))
            }

            "jmpif" => {
                let cond = self.parse_operand()?;
                self.expect(&Token::Comma)?;
                let label = self.expect_ident()?;
                Ok(Instruction::JmpIf(cond, label))
            }

            "jmpifnot" => {
                let cond = self.parse_operand()?;
                self.expect(&Token::Comma)?;
                let label = self.expect_ident()?;
                Ok(Instruction::JmpIfNot(cond, label))
            }

            "call" => {
                let dst = self.expect_ident()?;
                self.expect(&Token::Comma)?;
                let func = self.expect_ident()?;
                let mut args = Vec::new();
                while *self.peek() == Token::Comma {
                    self.advance();
                    args.push(self.parse_operand()?);
                }
                Ok(Instruction::Call(dst, func, args))
            }

            "call.ext" => {
                let func = self.expect_ident()?;
                let mut args = Vec::new();
                while *self.peek() == Token::Comma {
                    self.advance();
                    args.push(self.parse_operand()?);
                }
                Ok(Instruction::CallExt(func, args))
            }

            "call.void" => {
                let func = self.expect_ident()?;
                let mut args = Vec::new();
                while *self.peek() == Token::Comma {
                    self.advance();
                    args.push(self.parse_operand()?);
                }
                Ok(Instruction::CallVoid(func, args))
            }

            "call.ext.void" => {
                let func = self.expect_ident()?;
                let mut args = Vec::new();
                while *self.peek() == Token::Comma {
                    self.advance();
                    args.push(self.parse_operand()?);
                }
                Ok(Instruction::CallExtVoid(func, args))
            }

            "ret" => {
                match self.peek() {
                    Token::Newline | Token::Eof => Ok(Instruction::Ret(None)),
                    _ => Ok(Instruction::Ret(Some(self.parse_operand()?))),
                }
            }

            "print"       => Ok(Instruction::Print(self.parse_operand()?)),
            "printint"    => Ok(Instruction::PrintInt(self.parse_operand()?)),
            "printfloat"  => Ok(Instruction::PrintFloat(self.parse_operand()?)),
            "printchar"   => Ok(Instruction::PrintChar(self.parse_operand()?)),
            "printptr"    => Ok(Instruction::PrintPtr(self.parse_operand()?)),

            "timens"     => Ok(Instruction::TimeNs(self.expect_ident()?)),
            "timems"     => Ok(Instruction::TimeMs(self.expect_ident()?)),
            "timemonons" => Ok(Instruction::TimeMonoNs(self.expect_ident()?)),

            // extended arithmetic - binary
            "mod"    => self.parse_binary_op(Instruction::ModInt),
            "modf"   => self.parse_binary_op(Instruction::ModFloat),
            "pow"    => self.parse_binary_op(Instruction::PowInt),
            "powf"   => self.parse_binary_op(Instruction::PowFloat),

            // extended arithmetic - unary
            "neg"    => self.parse_unary_op(Instruction::NegInt),
            "negf"   => self.parse_unary_op(Instruction::NegFloat),
            "abs"    => self.parse_unary_op(Instruction::AbsInt),
            "absf"   => self.parse_unary_op(Instruction::AbsFloat),
            "sqrt"   => self.parse_unary_op(Instruction::SqrtFloat),

            // type casts
            "itof"   => self.parse_unary_op(Instruction::IntToFloat),
            "ftoi"   => self.parse_unary_op(Instruction::FloatToInt),
            "itoc"   => self.parse_unary_op(Instruction::IntToChar),
            "ctoi"   => self.parse_unary_op(Instruction::CharToInt),
            "ptoi"   => self.parse_unary_op(Instruction::PtrToInt),

            // string / char ops
            "strlen"   => self.parse_unary_op(Instruction::StrLen),
            "streq"    => self.parse_binary_op(Instruction::StrEq),
            "charat"   => self.parse_binary_op(Instruction::StrCharAt),
            "upper"    => self.parse_unary_op(Instruction::CharToUpper),
            "lower"    => self.parse_unary_op(Instruction::CharToLower),
            "itos"     => self.parse_unary_op(Instruction::IntToStr),
            "ftos"     => self.parse_unary_op(Instruction::FloatToStr),

            // arrays
            "arr.new" => {
                let dst = self.expect_ident()?;
                self.expect(&Token::Comma)?;
                let size = self.parse_operand()?;
                Ok(Instruction::ArrNew(dst, size))
            }
            "arr.get" => self.parse_binary_op(|dst, arr, idx| Instruction::ArrGet(dst, arr, idx)),
            "arr.set" => {
                let arr = self.parse_operand()?;
                self.expect(&Token::Comma)?;
                let idx = self.parse_operand()?;
                self.expect(&Token::Comma)?;
                let val = self.parse_operand()?;
                Ok(Instruction::ArrSet(arr, idx, val))
            }
            "arr.len"  => self.parse_unary_op(Instruction::ArrLen),
            "arr.free" => Ok(Instruction::ArrFree(self.parse_operand()?)),

            // stdin input
            "readchar"  => Ok(Instruction::ReadChar(self.expect_ident()?)),
            "readint"   => Ok(Instruction::ReadInt(self.expect_ident()?)),
            "readfloat" => Ok(Instruction::ReadFloat(self.expect_ident()?)),
            "readline"  => Ok(Instruction::ReadLine(self.expect_ident()?)),

            // bitwise
            "and"  => self.parse_binary_op(Instruction::BitAnd),
            "or"   => self.parse_binary_op(Instruction::BitOr),
            "xor"  => self.parse_binary_op(Instruction::BitXor),
            "not"  => self.parse_unary_op(Instruction::BitNot),
            "shl"  => self.parse_binary_op(Instruction::Shl),
            "shr"  => self.parse_binary_op(Instruction::Shr),

            // function pointers
            "func.ptr" => {
                let dst = self.expect_ident()?;
                self.expect(&Token::Comma)?;
                let name = self.expect_ident()?;
                Ok(Instruction::FuncPtr(dst, name))
            }
            "call.ptr" => {
                let dst = self.expect_ident()?;
                self.expect(&Token::Comma)?;
                let fptr = self.parse_operand()?;
                let mut args = Vec::new();
                while *self.peek() == Token::Comma {
                    self.advance();
                    args.push(self.parse_operand()?);
                }
                Ok(Instruction::CallPtr(dst, fptr, args))
            }
            "call.ptr.void" => {
                let fptr = self.parse_operand()?;
                let mut args = Vec::new();
                while *self.peek() == Token::Comma {
                    self.advance();
                    args.push(self.parse_operand()?);
                }
                Ok(Instruction::CallPtrVoid(fptr, args))
            }
            "panic" => Ok(Instruction::Panic(self.parse_operand()?)),

            _ => Err(AsmError::UnknownInstruction(mnemonic, span)),
        }
    }

    fn parse_func_body(&mut self) -> Result<(Vec<Register>, Vec<Instruction>)> {
        let mut locals = Vec::new();
        let mut body = Vec::new();

        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::Endfunc | Token::Eof => break,

                // local register declaration
                Token::TyInt | Token::TyBool | Token::TyFloat | Token::TyChar | Token::TyPtr => {
                    let ty = self.parse_type()?;
                    let name = self.expect_ident()?;
                    locals.push(Register { name, ty });
                }

                Token::Ident(s) => {
                    let s = s.clone();
                    let span = self.span();
                    self.advance();

                    // bare ident on its own line = label, unless it's a known mnemonic
                    let is_mnemonic = matches!(s.as_str(),
                        "add" | "sub" | "mul" | "div" |
                        "eq" | "ne" | "lt" | "le" | "gt" | "ge" |
                        "load" | "alloc" | "free" | "store" | "read" |
                        "jmp" | "jmpif" | "jmpifnot" |
                        "call" | "call.ext" | "call.void" | "call.ext.void" |
                        "ret" |
                        "print" | "printint" | "printfloat" | "printchar" | "printptr" |
                        "timens" | "timems" | "timemonons" |
                        "mod" | "modf" | "pow" | "powf" |
                        "neg" | "negf" | "abs" | "absf" | "sqrt" |
                        "itof" | "ftoi" | "itoc" | "ctoi" | "ptoi" |
                        "strlen" | "streq" | "charat" | "upper" | "lower" | "itos" | "ftos" |
                        "arr.new" | "arr.get" | "arr.set" | "arr.len" | "arr.free" |
                        "readchar" | "readint" | "readfloat" | "readline" |
                        "and" | "or" | "xor" | "not" | "shl" | "shr" |
                        "func.ptr" | "call.ptr" | "call.ptr.void" | "panic"
                    );
                    if !is_mnemonic && matches!(self.peek(), Token::Newline | Token::Eof) {
                        body.push(Instruction::Label(s));
                    } else {
                        body.push(self.parse_instruction(s, span)?);
                    }
                }

                t => return Err(AsmError::UnexpectedToken(format!("{:?}", t), self.span())),
            }
        }

        Ok((locals, body))
    }

    fn parse_func(&mut self) -> Result<FuncDecl> {
        let name = self.expect_ident()?;
        let params = self.parse_params()?;
        self.expect(&Token::Arrow)?;
        let ret_type = self.parse_type()?;
        self.skip_newlines();
        let (locals, body) = self.parse_func_body()?;
        self.expect(&Token::Endfunc)?;
        Ok(FuncDecl { name, params, ret_type, locals, body })
    }

    fn parse_extern(&mut self) -> Result<ExternDecl> {
        let name = self.expect_ident()?;
        let (params, variadic) = self.parse_extern_params()?;
        self.expect(&Token::Arrow)?;
        let ret_type = self.parse_type()?;
        Ok(ExternDecl { name, params, ret_type, variadic })
    }

    fn parse_data_section(&mut self) -> Result<Vec<DataItem>> {
        let mut items = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::DirStr => {
                    self.advance();
                    let name = self.expect_ident()?;
                    let span = self.span();
                    match self.advance() {
                        Token::LitString(s) => items.push(DataItem::Str(name, s)),
                        t => return Err(AsmError::UnexpectedToken(format!("{:?}", t), span)),
                    }
                }
                Token::DirI64 => {
                    self.advance();
                    let name = self.expect_ident()?;
                    let span = self.span();
                    match self.advance() {
                        Token::LitInt(n) => items.push(DataItem::Int(name, n)),
                        t => return Err(AsmError::UnexpectedToken(format!("{:?}", t), span)),
                    }
                }
                Token::Section | Token::Func | Token::Extern | Token::Eof => break,
                t => return Err(AsmError::UnexpectedToken(format!("{:?}", t), self.span())),
            }
        }
        Ok(items)
    }

    pub fn parse(&mut self) -> Result<Program> {
        let mut program = Program::default();
        loop {
            self.skip_newlines();
            match self.peek().clone() {
                Token::Eof => break,

                Token::Extern => {
                    self.advance();
                    let ext = self.parse_extern()?;
                    if program.externs.iter().any(|e| e.name == ext.name) {
                        return Err(AsmError::DuplicateExtern(ext.name, self.span()));
                    }
                    program.externs.push(ext);
                }

                Token::Section => {
                    self.advance();
                    let span = self.span();
                    match self.advance() {
                        Token::SecData => {
                            self.skip_newlines();
                            let items = self.parse_data_section()?;
                            program.data.extend(items);
                        }
                        Token::SecCode => {}
                        t => return Err(AsmError::UnexpectedToken(format!("{:?}", t), span)),
                    }
                }

                Token::Func => {
                    self.advance();
                    let func = self.parse_func()?;
                    if program.funcs.iter().any(|f| f.name == func.name) {
                        return Err(AsmError::DuplicateFunc(func.name, self.span()));
                    }
                    program.funcs.push(func);
                }

                t => return Err(AsmError::UnexpectedToken(format!("{:?}", t), self.span())),
            }
        }
        Ok(program)
    }
}
