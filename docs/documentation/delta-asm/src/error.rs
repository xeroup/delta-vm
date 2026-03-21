use std::fmt;

#[derive(Debug, Clone)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

const RED: &str = "\x1b[31m";
const WHITE: &str = "\x1b[37m";
const YELLOW: &str = "\x1b[33m";
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";

#[derive(Debug, Clone)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub severity: Severity,
    pub span: Option<Span>,
    pub message: String,
}

impl Diagnostic {
    pub fn error(span: Span, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, span: Some(span), message: message.into() }
    }

    pub fn error_no_span(message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, span: None, message: message.into() }
    }

    pub fn warning(span: Span, message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, span: Some(span), message: message.into() }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.severity {
            Severity::Error => match &self.span {
                Some(s) => write!(f, "{RED}{BOLD}error{RESET}{BOLD}:{RESET} {WHITE}{}{RESET} {RED}(at {s}){RESET}", self.message),
                None    => write!(f, "{RED}{BOLD}error{RESET}{BOLD}:{RESET} {WHITE}{}{RESET}", self.message),
            },
            Severity::Warning => match &self.span {
                Some(s) => write!(f, "{YELLOW}{BOLD}warning{RESET}{BOLD}:{RESET} {WHITE}{}{RESET} {YELLOW}(at {s}){RESET}", self.message),
                None    => write!(f, "{YELLOW}{BOLD}warning{RESET}{BOLD}:{RESET} {WHITE}{}{RESET}", self.message),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum AsmError {
    UnexpectedChar(char, Span),
    UnexpectedToken(String, Span),
    UnexpectedEof,
    UnknownType(String, Span),
    UnknownInstruction(String, Span),
    InvalidLiteral(String, Span),
    DuplicateFunc(String, Span),
    DuplicateExtern(String, Span),
}

impl AsmError {
    pub fn to_diagnostic(&self) -> Diagnostic {
        match self {
            AsmError::UnexpectedChar(c, s) =>
                Diagnostic::error(s.clone(), format!("unexpected character '{c}'")),
            AsmError::UnexpectedToken(t, s) =>
                Diagnostic::error(s.clone(), format!("unexpected token '{t}'")),
            AsmError::UnexpectedEof =>
                Diagnostic::error_no_span("unexpected end of file"),
            AsmError::UnknownType(t, s) =>
                Diagnostic::error(s.clone(), format!("unknown type '{t}'")),
            AsmError::UnknownInstruction(i, s) =>
                Diagnostic::error(s.clone(), format!("unknown instruction '{i}'")),
            AsmError::InvalidLiteral(l, s) =>
                Diagnostic::error(s.clone(), format!("invalid literal '{l}'")),
            AsmError::DuplicateFunc(n, s) =>
                Diagnostic::error(s.clone(), format!("duplicate function '{n}'")),
            AsmError::DuplicateExtern(n, s) =>
                Diagnostic::error(s.clone(), format!("duplicate extern '{n}'")),
        }
    }
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_diagnostic())
    }
}

pub type Result<T> = std::result::Result<T, AsmError>;
