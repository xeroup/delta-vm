// delta-cranelift: compiles DcFile to native code via Cranelift
//
// pipeline:
//   DcFile -> Cranelift IR -> optimize -> emit object -> link -> ELF/Mach-O/PE
//
// public API is intentionally identical to the old delta-llvm crate

pub mod codegen;
pub mod link;

use delta_format::file::DcFile;

#[derive(Debug)]
pub struct CraneliftError(pub String);

impl std::fmt::Display for CraneliftError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cranelift error: {}", self.0)
    }
}

pub type Result<T> = std::result::Result<T, CraneliftError>;

pub struct CompileOptions {
    pub entry: String,
    pub opt_level: OptLevel,
    pub emit: EmitKind,
    // windows only: use SUBSYSTEM:WINDOWS instead of SUBSYSTEM:CONSOLE
    pub no_console: bool,
}

#[derive(Clone, Copy)]
pub enum OptLevel {
    None,
    Less,
    Default,
    Aggressive,
}

#[derive(Clone, Copy, PartialEq)]
pub enum EmitKind {
    Object,
    Asm,
    Exe,
}

pub fn compile(dc: &DcFile, opts: &CompileOptions) -> Result<Vec<u8>> {
    let obj = codegen::compile_object(dc, &opts.entry, opts.opt_level)?;
    match opts.emit {
        EmitKind::Object => Ok(obj),
        EmitKind::Exe => link::link_exe(&obj, opts.no_console),
        EmitKind::Asm => Err(CraneliftError(
            "asm emit not supported; use --emit obj".into(),
        )),
    }
}
