use crate::error::{AsmError, Result, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    // directives
    Func,
    Endfunc,
    Extern,
    Section,

    // types
    TyInt,
    TyBool,
    TyFloat,
    TyChar,
    TyPtr,
    TyVoid,

    // section names
    SecCode,
    SecData,

    // data directives
    DirStr,
    DirI64,

    // symbols
    Arrow,
    Comma,
    Lparen,
    Rparen,
    At,
    Ellipsis,  // ...

    // values
    Ident(String),
    LitInt(i64),
    LitFloat(f64),
    LitChar(char),
    LitString(String),

    Newline,
    Eof,
}

pub struct Lexer<'a> {
    src: &'a str,
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self { src, pos: 0, line: 1, col: 1 }
    }

    fn span(&self) -> Span {
        Span { line: self.line, col: self.col }
    }

    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn peek2(&self) -> Option<char> {
        let mut it = self.src[self.pos..].chars();
        it.next();
        it.next()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_inline_whitespace(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\r')) {
            self.advance();
        }
    }

    fn skip_comment(&mut self) {
        while !matches!(self.peek(), Some('\n') | None) {
            self.advance();
        }
    }

    fn read_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' || c == '.' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_digits(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }
        s
    }

    fn read_number(&mut self, negative: bool, span: Span) -> Result<Token> {
        let mut s = read_digits(self);
        let mut is_float = false;

        if self.peek() == Some('.') && self.peek2().map_or(false, |c| c.is_ascii_digit()) {
            is_float = true;
            s.push('.');
            self.advance();
            s.push_str(&read_digits(self));
        }

        if negative {
            s.insert(0, '-');
        }

        if is_float {
            s.parse::<f64>()
                .map(Token::LitFloat)
                .map_err(|_| AsmError::InvalidLiteral(s, span))
        } else {
            s.parse::<i64>()
                .map(Token::LitInt)
                .map_err(|_| AsmError::InvalidLiteral(s, span))
        }
    }

    fn read_char_literal(&mut self, span: Span) -> Result<Token> {
        let c = self.advance().ok_or(AsmError::UnexpectedEof)?;
        let ch = if c == '\\' {
            match self.advance().ok_or(AsmError::UnexpectedEof)? {
                'n' => '\n', 't' => '\t', 'r' => '\r',
                '0' => '\0', '\'' => '\'', '\\' => '\\',
                e => return Err(AsmError::InvalidLiteral(format!("\\{}", e), span)),
            }
        } else {
            c
        };
        match self.advance() {
            Some('\'') => Ok(Token::LitChar(ch)),
            _ => Err(AsmError::InvalidLiteral("unclosed char literal".into(), span)),
        }
    }

    fn read_string_literal(&mut self, span: Span) -> Result<Token> {
        let mut s = String::new();
        loop {
            match self.advance() {
                None => return Err(AsmError::UnexpectedEof),
                Some('"') => break,
                Some('\\') => {
                    match self.advance().ok_or(AsmError::UnexpectedEof)? {
                        'n' => s.push('\n'), 't' => s.push('\t'), 'r' => s.push('\r'),
                        '0' => s.push('\0'), '"' => s.push('"'), '\\' => s.push('\\'),
                        e => return Err(AsmError::InvalidLiteral(format!("\\{}", e), span)),
                    }
                }
                Some(c) => s.push(c),
            }
        }
        Ok(Token::LitString(s))
    }

    fn keyword_or_ident(s: String) -> Token {
        match s.as_str() {
            ".func" => Token::Func,
            ".endfunc" => Token::Endfunc,
            ".extern" => Token::Extern,
            ".section" => Token::Section,
            ".str" => Token::DirStr,
            ".i64" => Token::DirI64,
            "int" => Token::TyInt,
            "bool" => Token::TyBool,
            "float" => Token::TyFloat,
            "char" => Token::TyChar,
            "ptr" => Token::TyPtr,
            "void" => Token::TyVoid,
            "ret" => Token::Ident("ret".into()),
            "code" => Token::SecCode,
            "data" => Token::SecData,
            _ => Token::Ident(s),
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<(Token, Span)>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_inline_whitespace();
            let span = self.span();
            match self.peek() {
                None => { tokens.push((Token::Eof, span)); break; }
                Some(';') => self.skip_comment(),
                Some('\n') => { self.advance(); tokens.push((Token::Newline, span)); }
                Some(',') => { self.advance(); tokens.push((Token::Comma, span)); }
                Some('(') => { self.advance(); tokens.push((Token::Lparen, span)); }
                Some(')') => { self.advance(); tokens.push((Token::Rparen, span)); }
                Some('@') => { self.advance(); tokens.push((Token::At, span)); }
                Some('-') if self.peek2() == Some('>') => {
                    self.advance(); self.advance();
                    tokens.push((Token::Arrow, span));
                }
                Some('.') if self.src[self.pos..].starts_with("...") => {
                    self.advance(); self.advance(); self.advance();
                    tokens.push((Token::Ellipsis, span));
                }
                Some('-') => {
                    self.advance();
                    let tok = self.read_number(true, span.clone())?;
                    tokens.push((tok, span));
                }
                Some('\'') => {
                    self.advance();
                    let tok = self.read_char_literal(span.clone())?;
                    tokens.push((tok, span));
                }
                Some('"') => {
                    self.advance();
                    let tok = self.read_string_literal(span.clone())?;
                    tokens.push((tok, span));
                }
                Some(c) if c.is_ascii_digit() => {
                    let tok = self.read_number(false, span.clone())?;
                    tokens.push((tok, span));
                }
                Some(c) if c.is_alphabetic() || c == '_' || c == '.' => {
                    let s = self.read_ident();
                    tokens.push((Self::keyword_or_ident(s), span));
                }
                Some(c) => return Err(AsmError::UnexpectedChar(c, span)),
            }
        }
        Ok(tokens)
    }
}

// helper to read digits without borrowing self in method
fn read_digits(l: &mut Lexer) -> String {
    let mut s = String::new();
    while l.peek().map_or(false, |c| c.is_ascii_digit()) {
        s.push(l.peek().unwrap());
        l.advance();
    }
    s
}
