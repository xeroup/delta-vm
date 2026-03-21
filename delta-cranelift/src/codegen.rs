// compiles a DcFile into a native .o using Cranelift
//
// type model: all delta values are i64
//   floats  - bitcast i64 <-> f64 at use sites
//   chars   - i64 (low 32 bits)
//   ptrs    - i64

use cranelift_codegen::{
    ir::{
        self,
        condcodes::{FloatCC, IntCC},
        types::{F64, I64, I8},
        AbiParam, Block, Function, InstBuilder, MemFlags, TrapCode, UserFuncName,
    },
    settings::{self, Configurable},
    Context,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use delta_format::{
    encoding::{decode_func, Instr},
    file::{DataEntry, DcFile},
    opcode::Op,
};

use crate::{CraneliftError, OptLevel, Result};

pub fn compile_object(dc: &DcFile, entry: &str, opt: OptLevel) -> Result<Vec<u8>> {
    let mut flag_builder = settings::builder();
    let speed = match opt {
        OptLevel::None => "none",
        OptLevel::Less | OptLevel::Default => "speed",
        OptLevel::Aggressive => "speed_and_size",
    };
    flag_builder.set("opt_level", speed).unwrap();
    flag_builder.set("enable_verifier", "false").unwrap();
    let flags = settings::Flags::new(flag_builder);

    let isa = cranelift_native::builder()
        .map_err(|e| CraneliftError(format!("native isa: {e}")))?
        .finish(flags)
        .map_err(|e| CraneliftError(format!("isa finish: {e}")))?;

    let obj_builder = ObjectBuilder::new(
        isa,
        "delta",
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| CraneliftError(format!("obj builder: {e}")))?;

    let mut module = ObjectModule::new(obj_builder);

    let ext_ids = declare_externs(dc, &mut module)?;
    let func_ids = declare_delta_funcs(dc, entry, &mut module)?;
    let data_ids = emit_data_section(dc, &mut module)?;

    for (fi, f) in dc.funcs.iter().enumerate() {
        let instrs = decode_func(&dc.code, f.code_offset as usize, f.code_len as usize)
            .ok_or_else(|| CraneliftError(format!("decode failed for '{}'", f.name)))?;
        compile_func(
            dc, &mut module, &func_ids, &ext_ids, &data_ids,
            fi, f.reg_count as usize, f.param_count as usize, &instrs,
        )?;
    }

    let product = module.finish();
    product.emit().map_err(|e| CraneliftError(format!("emit: {e}")))
}

fn delta_sig(module: &ObjectModule, param_count: usize, returns: bool) -> ir::Signature {
    let mut sig = module.make_signature();
    for _ in 0..param_count { sig.params.push(AbiParam::new(I64)); }
    if returns { sig.returns.push(AbiParam::new(I64)); }
    sig
}

fn declare_externs(dc: &DcFile, module: &mut ObjectModule) -> Result<Vec<FuncId>> {
    let mut ids = Vec::new();
    for ext in &dc.externs {
        let mut sig = module.make_signature();
        for _ in 0..ext.param_count { sig.params.push(AbiParam::new(I64)); }
        sig.returns.push(AbiParam::new(I64));
        let id = module
            .declare_function(&ext.name, Linkage::Import, &sig)
            .map_err(|e| CraneliftError(format!("declare extern '{}': {e}", ext.name)))?;
        ids.push(id);
    }
    Ok(ids)
}

fn declare_delta_funcs(dc: &DcFile, entry: &str, module: &mut ObjectModule) -> Result<Vec<FuncId>> {
    let mut ids = Vec::new();
    for f in &dc.funcs {
        let name = if f.name == entry { "main".to_string() } else { format!("delta_{}", f.name) };
        let sig = delta_sig(module, f.param_count as usize, true);
        let id = module
            .declare_function(&name, Linkage::Export, &sig)
            .map_err(|e| CraneliftError(format!("declare func '{}': {e}", f.name)))?;
        ids.push(id);
    }
    Ok(ids)
}

fn emit_data_section(dc: &DcFile, module: &mut ObjectModule) -> Result<Vec<cranelift_module::DataId>> {
    let mut ids = Vec::new();
    for (i, item) in dc.data.iter().enumerate() {
        let data_id = module
            .declare_data(&format!("delta_data_{i}"), Linkage::Local, false, false)
            .map_err(|e| CraneliftError(format!("declare data {i}: {e}")))?;
        let mut desc = DataDescription::new();
        match item {
            DataEntry::Str(b) => desc.define(b.clone().into_boxed_slice()),
            DataEntry::Int(n) => desc.define(n.to_le_bytes().to_vec().into_boxed_slice()),
            DataEntry::Float(f) => desc.define(f.to_bits().to_le_bytes().to_vec().into_boxed_slice()),
        }
        module.define_data(data_id, &desc)
            .map_err(|e| CraneliftError(format!("define data {i}: {e}")))?;
        ids.push(data_id);
    }
    Ok(ids)
}

// declare a libc/runtime function and return a local FuncRef
fn decl_rt(
    module: &mut ObjectModule,
    func: &mut Function,
    name: &str,
    params: &[ir::Type],
    rets: &[ir::Type],
) -> Result<ir::FuncRef> {
    let mut sig = module.make_signature();
    for &t in params { sig.params.push(AbiParam::new(t)); }
    for &t in rets   { sig.returns.push(AbiParam::new(t)); }
    let fid = module
        .declare_function(name, Linkage::Import, &sig)
        .map_err(|e| CraneliftError(format!("declare rt '{name}': {e}")))?;
    Ok(module.declare_func_in_func(fid, func))
}

fn compile_func(
    dc: &DcFile,
    module: &mut ObjectModule,
    func_ids: &[FuncId],
    ext_ids: &[FuncId],
    data_ids: &[cranelift_module::DataId],
    func_idx: usize,
    reg_count: usize,
    param_count: usize,
    instrs: &[Instr],
) -> Result<()> {
    let f_info = &dc.funcs[func_idx];
    let sig = delta_sig(module, param_count, true);
    let mut func = Function::with_name_signature(UserFuncName::user(0, func_idx as u32), sig);

    let mut fb_ctx = FunctionBuilderContext::new();
    let mut b = FunctionBuilder::new(&mut func, &mut fb_ctx);

    let regs: Vec<Variable> = (0..reg_count).map(|i| Variable::from_u32(i as u32)).collect();
    for &v in &regs { b.declare_var(v, I64); }

    let entry_block = b.create_block();
    b.append_block_params_for_function_params(entry_block);
    b.switch_to_block(entry_block);
    b.seal_block(entry_block);

    // init params from function args
    let params: Vec<ir::Value> = b.block_params(entry_block).to_vec();
    for (i, val) in params.into_iter().enumerate() {
        b.def_var(regs[i], val);
    }
    let zero = b.ins().iconst(I64, 0);
    for i in param_count..reg_count { b.def_var(regs[i], zero); }

    // pre-create blocks for jump targets
    let mut blocks: Vec<Option<Block>> = vec![None; instrs.len() + 1];
    for (ii, instr) in instrs.iter().enumerate() {
        match instr {
            Instr::B { op: Op::Jmp, imm, .. } => {
                let t = *imm as usize;
                if blocks[t].is_none() { blocks[t] = Some(b.create_block()); }
            }
            Instr::B { op: Op::JmpIf, imm, .. } |
            Instr::B { op: Op::JmpIfNot, imm, .. } => {
                let t = *imm as usize;
                let n = ii + 1;
                if blocks[t].is_none() { blocks[t] = Some(b.create_block()); }
                if n < blocks.len() && blocks[n].is_none() {
                    blocks[n] = Some(b.create_block());
                }
            }
            _ => {}
        }
    }

    let mut terminated = false;
    for (ii, instr) in instrs.iter().enumerate() {
        if let Some(blk) = blocks[ii] {
            if !terminated { b.ins().jump(blk, &[]); }
            b.switch_to_block(blk);
            b.seal_block(blk);
            terminated = false;
        }
        if terminated { continue; }

        terminated = emit_instr(
            instr, ii, &mut b, &regs, &blocks,
            module, func_ids, ext_ids, data_ids,
        )?;
    }

    if !terminated {
        let zero = b.ins().iconst(I64, 0);
        b.ins().return_(&[zero]);
    }

    b.finalize();

    let mut ctx = Context::for_function(func);
    module
        .define_function(func_ids[func_idx], &mut ctx)
        .map_err(|e| CraneliftError(format!("define func '{}': {e}", f_info.name)))?;

    Ok(())
}

// get/set register helpers - explicit let bindings avoid double-borrow issues
macro_rules! rg {
    ($b:expr, $regs:expr, $r:expr) => { $b.use_var($regs[$r as usize]) };
}
macro_rules! rs {
    ($b:expr, $regs:expr, $r:expr, $v:expr) => { $b.def_var($regs[$r as usize], $v) };
}
// bitcast i64<->f64 stored as i64
macro_rules! as_f {
    ($b:expr, $v:expr) => { $b.ins().bitcast(F64, MemFlags::new(), $v) };
}
macro_rules! as_i {
    ($b:expr, $v:expr) => { $b.ins().bitcast(I64, MemFlags::new(), $v) };
}

// create (or reuse) a read-only string literal symbol
fn str_gv(module: &mut ObjectModule, b: &mut FunctionBuilder, bytes: &[u8]) -> Result<ir::GlobalValue> {
    let hash: u64 = bytes.iter().fold(0u64, |h, &x| h.wrapping_mul(31).wrapping_add(x as u64));
    let name = format!("delta_str_{hash:016x}");
    let data_id = module
        .declare_data(&name, Linkage::Local, false, false)
        .map_err(|e| CraneliftError(format!("declare str: {e}")))?;
    let mut desc = DataDescription::new();
    desc.define(bytes.to_vec().into_boxed_slice());
    let _ = module.define_data(data_id, &desc);
    Ok(module.declare_data_in_func(data_id, b.func))
}

// returns true if the instruction terminates the block
#[allow(clippy::too_many_arguments)]
fn emit_instr(
    instr: &Instr,
    ii: usize,
    b: &mut FunctionBuilder,
    regs: &[Variable],
    blocks: &[Option<Block>],
    module: &mut ObjectModule,
    func_ids: &[FuncId],
    ext_ids: &[FuncId],
    data_ids: &[cranelift_module::DataId],
) -> Result<bool> {
    match instr {
        // --- integer arithmetic ---
        Instr::A { op: Op::AddInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().iadd(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::SubInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().isub(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::MulInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().imul(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::DivInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().sdiv(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::ModInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().srem(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::NegInt, dst, a, .. } => {
            let va = rg!(b, regs, *a);
            let r = b.ins().ineg(va);
            rs!(b, regs, *dst, r);
        }
        Instr::D { op: Op::NegInt, src } => {
            let va = rg!(b, regs, *src);
            let r = b.ins().ineg(va);
            rs!(b, regs, *src, r);
        }
        Instr::D { op: Op::AbsInt, src } => {
            let va = rg!(b, regs, *src);
            let f = decl_rt(module, b.func, "llabs", &[I64], &[I64])?;
            let call = b.ins().call(f, &[va]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *src, r);
        }
        Instr::A { op: Op::PowInt, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va);
            let fb = as_f!(b, vb);
            let ia = as_i!(b, fa);
            let ib = as_i!(b, fb);
            let f = decl_rt(module, b.func, "pow", &[I64, I64], &[I64])?;
            let call = b.ins().call(f, &[ia, ib]);
            let tmp_r = b.inst_results(call)[0];
            let fres = as_f!(b, tmp_r);
            let r = b.ins().fcvt_to_sint(I64, fres);
            rs!(b, regs, *dst, r);
        }

        // --- float arithmetic ---
        Instr::A { op: Op::AddFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va);
            let fb = as_f!(b, vb);
            let r = b.ins().fadd(fa, fb);
            let ri = as_i!(b, r);
            rs!(b, regs, *dst, ri);
        }
        Instr::A { op: Op::SubFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va);
            let fb = as_f!(b, vb);
            let r = b.ins().fsub(fa, fb);
            let ri = as_i!(b, r);
            rs!(b, regs, *dst, ri);
        }
        Instr::A { op: Op::MulFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va);
            let fb = as_f!(b, vb);
            let r = b.ins().fmul(fa, fb);
            let ri = as_i!(b, r);
            rs!(b, regs, *dst, ri);
        }
        Instr::A { op: Op::DivFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va);
            let fb = as_f!(b, vb);
            let r = b.ins().fdiv(fa, fb);
            let ri = as_i!(b, r);
            rs!(b, regs, *dst, ri);
        }
        Instr::A { op: Op::ModFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let f = decl_rt(module, b.func, "fmod", &[I64, I64], &[I64])?;
            let call = b.ins().call(f, &[va, vb]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::PowFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let f = decl_rt(module, b.func, "pow", &[I64, I64], &[I64])?;
            let call = b.ins().call(f, &[va, vb]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::NegFloat, dst, a, .. } => {
            let va = rg!(b, regs, *a);
            let fa = as_f!(b, va);
            let r = b.ins().fneg(fa);
            let ri = as_i!(b, r);
            rs!(b, regs, *dst, ri);
        }
        Instr::D { op: Op::NegFloat, src } => {
            let va = rg!(b, regs, *src);
            let fa = as_f!(b, va);
            let r = b.ins().fneg(fa);
            let ri = as_i!(b, r);
            rs!(b, regs, *src, ri);
        }
        Instr::D { op: Op::AbsFloat, src } => {
            let va = rg!(b, regs, *src);
            let fa = as_f!(b, va);
            let r = b.ins().fabs(fa);
            let ri = as_i!(b, r);
            rs!(b, regs, *src, ri);
        }
        Instr::D { op: Op::SqrtFloat, src } => {
            let va = rg!(b, regs, *src);
            let fa = as_f!(b, va);
            let r = b.ins().sqrt(fa);
            let ri = as_i!(b, r);
            rs!(b, regs, *src, ri);
        }

        // --- integer comparisons ---
        Instr::A { op: Op::EqInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::Equal, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::NeInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::NotEqual, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::LtInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::SignedLessThan, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::LeInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::SignedLessThanOrEqual, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::GtInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::SignedGreaterThan, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::GeInt, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::SignedGreaterThanOrEqual, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::EqChar, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::Equal, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::NeChar, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().icmp(IntCC::NotEqual, va, vb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }

        // --- float comparisons ---
        Instr::A { op: Op::EqFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a); let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va); let fb = as_f!(b, vb);
            let r = b.ins().fcmp(FloatCC::Equal, fa, fb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::NeFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a); let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va); let fb = as_f!(b, vb);
            let r = b.ins().fcmp(FloatCC::NotEqual, fa, fb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::LtFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a); let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va); let fb = as_f!(b, vb);
            let r = b.ins().fcmp(FloatCC::LessThan, fa, fb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::LeFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a); let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va); let fb = as_f!(b, vb);
            let r = b.ins().fcmp(FloatCC::LessThanOrEqual, fa, fb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::GtFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a); let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va); let fb = as_f!(b, vb);
            let r = b.ins().fcmp(FloatCC::GreaterThan, fa, fb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::GeFloat, dst, a, b: rb } => {
            let va = rg!(b, regs, *a); let vb = rg!(b, regs, *rb);
            let fa = as_f!(b, va); let fb = as_f!(b, vb);
            let r = b.ins().fcmp(FloatCC::GreaterThanOrEqual, fa, fb);
            let r = b.ins().uextend(I64, r);
            rs!(b, regs, *dst, r);
        }

        // --- loads ---
        Instr::B { op: Op::LoadInt, dst, imm } => {
            let v = b.ins().iconst(I64, *imm as i64);
            rs!(b, regs, *dst, v);
        }
        Instr::B { op: Op::LoadFloat, dst, imm } => {
            let bits = (f32::from_bits(*imm) as f64).to_bits() as i64;
            let v = b.ins().iconst(I64, bits);
            rs!(b, regs, *dst, v);
        }
        Instr::B { op: Op::LoadChar, dst, imm } => {
            let v = b.ins().iconst(I64, *imm as i64);
            rs!(b, regs, *dst, v);
        }
        Instr::B { op: Op::LoadPtr, dst, imm } => {
            let gv = module.declare_data_in_func(data_ids[*imm as usize], b.func);
            let v = b.ins().global_value(I64, gv);
            rs!(b, regs, *dst, v);
        }

        // --- memory ---
        Instr::B { op: Op::Alloc, dst, imm } => {
            let sz = b.ins().iconst(I64, *imm as i64);
            let f = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let call = b.ins().call(f, &[sz]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::AllocReg, dst, a, .. } => {
            let sz = rg!(b, regs, *a);
            let f = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let call = b.ins().call(f, &[sz]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *dst, r);
        }
        Instr::D { op: Op::Free, src } => {
            let ptr = rg!(b, regs, *src);
            let f = decl_rt(module, b.func, "free", &[I64], &[])?;
            b.ins().call(f, &[ptr]);
        }
        Instr::A { op: Op::Store, a, b: rb, .. } => {
            let ptr = rg!(b, regs, *a);
            let val = rg!(b, regs, *rb);
            b.ins().store(MemFlags::new(), val, ptr, 0);
        }
        Instr::A { op: Op::Read, dst, a, .. } => {
            let ptr = rg!(b, regs, *a);
            let v = b.ins().load(I64, MemFlags::new(), ptr, 0);
            rs!(b, regs, *dst, v);
        }

        // --- control flow (terminators) ---
        Instr::B { op: Op::Jmp, imm, .. } => {
            b.ins().jump(blocks[*imm as usize].unwrap(), &[]);
            return Ok(true);
        }
        Instr::B { op: Op::JmpIf, dst, imm } => {
            let cond = rg!(b, regs, *dst);
            let zero = b.ins().iconst(I64, 0);
            let flag = b.ins().icmp(IntCC::NotEqual, cond, zero);
            b.ins().brif(flag, blocks[*imm as usize].unwrap(), &[], blocks[ii + 1].unwrap(), &[]);
            return Ok(true);
        }
        Instr::B { op: Op::JmpIfNot, dst, imm } => {
            let cond = rg!(b, regs, *dst);
            let zero = b.ins().iconst(I64, 0);
            let flag = b.ins().icmp(IntCC::Equal, cond, zero);
            b.ins().brif(flag, blocks[*imm as usize].unwrap(), &[], blocks[ii + 1].unwrap(), &[]);
            return Ok(true);
        }

        // --- calls ---
        Instr::C { op: Op::Call, dst, func_idx: fi, args } => {
            let fref = module.declare_func_in_func(func_ids[*fi as usize], b.func);
            let av: Vec<ir::Value> = args.iter().map(|r| b.use_var(regs[*r as usize])).collect();
            let call = b.ins().call(fref, &av);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *dst, r);
        }
        Instr::C { op: Op::CallVoid, func_idx: fi, args, .. } => {
            let fref = module.declare_func_in_func(func_ids[*fi as usize], b.func);
            let av: Vec<ir::Value> = args.iter().map(|r| b.use_var(regs[*r as usize])).collect();
            b.ins().call(fref, &av);
        }
        Instr::C { op: Op::CallExt, dst, func_idx: fi, args } => {
            let fref = module.declare_func_in_func(ext_ids[*fi as usize], b.func);
            let av: Vec<ir::Value> = args.iter().map(|r| b.use_var(regs[*r as usize])).collect();
            let call = b.ins().call(fref, &av);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *dst, r);
        }
        Instr::C { op: Op::CallExtVoid, func_idx: fi, args, .. } => {
            let fref = module.declare_func_in_func(ext_ids[*fi as usize], b.func);
            let av: Vec<ir::Value> = args.iter().map(|r| b.use_var(regs[*r as usize])).collect();
            b.ins().call(fref, &av);
        }
        Instr::C { op: Op::CallPtr, dst, func_idx: fptr_reg, args } => {
            let fptr = b.use_var(regs[*fptr_reg as usize]);
            let av: Vec<ir::Value> = args.iter().map(|r| b.use_var(regs[*r as usize])).collect();
            let sig = delta_sig(module, args.len(), true);
            let sigref = b.import_signature(sig);
            let call = b.ins().call_indirect(sigref, fptr, &av);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *dst, r);
        }
        Instr::C { op: Op::CallPtrVoid, func_idx: fptr_reg, args, .. } => {
            let fptr = b.use_var(regs[*fptr_reg as usize]);
            let av: Vec<ir::Value> = args.iter().map(|r| b.use_var(regs[*r as usize])).collect();
            let sig = delta_sig(module, args.len(), false);
            let sigref = b.import_signature(sig);
            b.ins().call_indirect(sigref, fptr, &av);
        }

        // --- return (terminators) ---
        Instr::D { op: Op::Ret, src } => {
            let v = rg!(b, regs, *src);
            b.ins().return_(&[v]);
            return Ok(true);
        }
        Instr::D { op: Op::RetVoid, .. } => {
            let z = b.ins().iconst(I64, 0);
            b.ins().return_(&[z]);
            return Ok(true);
        }

        // --- print ---
        Instr::D { op: Op::PrintInt, src } => {
            let val = rg!(b, regs, *src);
            let gv = str_gv(module, b, b"%lld\n\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let f = decl_rt(module, b.func, "printf", &[I64, I64], &[I64])?;
            b.ins().call(f, &[fmt, val]);
        }
        Instr::D { op: Op::PrintFloat, src } => {
            let val = rg!(b, regs, *src);
            let gv = str_gv(module, b, b"%g\n\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let f = decl_rt(module, b.func, "printf", &[I64, I64], &[I64])?;
            b.ins().call(f, &[fmt, val]);
        }
        Instr::D { op: Op::PrintChar, src } => {
            let val = rg!(b, regs, *src);
            let f = decl_rt(module, b.func, "putchar", &[I64], &[I64])?;
            b.ins().call(f, &[val]);
        }
        Instr::D { op: Op::PrintPtr, src } => {
            let ptr = rg!(b, regs, *src);
            let gv = str_gv(module, b, b"%s\n\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let f = decl_rt(module, b.func, "printf", &[I64, I64], &[I64])?;
            b.ins().call(f, &[fmt, ptr]);
        }

        // --- type casts ---
        Instr::D { op: Op::IntToFloat, src } => {
            let vi = rg!(b, regs, *src);
            let vf = b.ins().fcvt_from_sint(F64, vi);
            let r = as_i!(b, vf);
            rs!(b, regs, *src, r);
        }
        Instr::D { op: Op::FloatToInt, src } => {
            let vi = rg!(b, regs, *src);
            let vf = as_f!(b, vi);
            let r = b.ins().fcvt_to_sint(I64, vf);
            rs!(b, regs, *src, r);
        }
        // chars and ptrs are already i64 - no-op
        Instr::D { op: Op::IntToChar, .. } |
        Instr::D { op: Op::CharToInt, .. } |
        Instr::D { op: Op::PtrToInt, .. } => {}

        // --- bitwise ---
        Instr::A { op: Op::BitAnd, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().band(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::BitOr, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().bor(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::BitXor, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().bxor(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::BitNot, dst, a, .. } => {
            let va = rg!(b, regs, *a);
            let r = b.ins().bnot(va);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::Shl, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().ishl(va, vb);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::Shr, dst, a, b: rb } => {
            let (va, vb) = (rg!(b, regs, *a), rg!(b, regs, *rb));
            let r = b.ins().sshr(va, vb);
            rs!(b, regs, *dst, r);
        }

        // --- string ops ---
        Instr::D { op: Op::StrLen, src } => {
            let ptr = rg!(b, regs, *src);
            let f = decl_rt(module, b.func, "strlen", &[I64], &[I64])?;
            let call = b.ins().call(f, &[ptr]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *src, r);
        }
        Instr::A { op: Op::StrEq, dst, a, b: rb } => {
            let va = rg!(b, regs, *a);
            let vb = rg!(b, regs, *rb);
            let f = decl_rt(module, b.func, "strcmp", &[I64, I64], &[I64])?;
            let call = b.ins().call(f, &[va, vb]);
            let cmp = b.inst_results(call)[0];
            let zero = b.ins().iconst(I64, 0);
            let eq = b.ins().icmp(IntCC::Equal, cmp, zero);
            let r = b.ins().uextend(I64, eq);
            rs!(b, regs, *dst, r);
        }
        Instr::A { op: Op::StrCharAt, dst, a, b: rb } => {
            let ptr = rg!(b, regs, *a);
            let idx = rg!(b, regs, *rb);
            let addr = b.ins().iadd(ptr, idx);
            let byte = b.ins().load(I8, MemFlags::new(), addr, 0);
            let r = b.ins().uextend(I64, byte);
            rs!(b, regs, *dst, r);
        }
        Instr::D { op: Op::CharToUpper, src } => {
            let v = rg!(b, regs, *src);
            let f = decl_rt(module, b.func, "toupper", &[I64], &[I64])?;
            let call = b.ins().call(f, &[v]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *src, r);
        }
        Instr::D { op: Op::CharToLower, src } => {
            let v = rg!(b, regs, *src);
            let f = decl_rt(module, b.func, "tolower", &[I64], &[I64])?;
            let call = b.ins().call(f, &[v]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *src, r);
        }
        Instr::D { op: Op::IntToStr, src } => {
            let val = rg!(b, regs, *src);
            let sz = b.ins().iconst(I64, 32);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let cm = b.ins().call(malloc, &[sz]);
            let buf = b.inst_results(cm)[0];
            let gv = str_gv(module, b, b"%lld\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let sprintf = decl_rt(module, b.func, "sprintf", &[I64, I64, I64], &[I64])?;
            b.ins().call(sprintf, &[buf, fmt, val]);
            rs!(b, regs, *src, buf);
        }
        Instr::D { op: Op::FloatToStr, src } => {
            let val = rg!(b, regs, *src);
            let sz = b.ins().iconst(I64, 64);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let cm = b.ins().call(malloc, &[sz]);
            let buf = b.inst_results(cm)[0];
            let gv = str_gv(module, b, b"%g\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let sprintf = decl_rt(module, b.func, "sprintf", &[I64, I64, I64], &[I64])?;
            b.ins().call(sprintf, &[buf, fmt, val]);
            rs!(b, regs, *src, buf);
        }

        // --- input ---
        Instr::D { op: Op::ReadChar, src } => {
            let f = decl_rt(module, b.func, "getchar", &[], &[I64])?;
            let call = b.ins().call(f, &[]);
            let r = b.inst_results(call)[0];
            rs!(b, regs, *src, r);
        }
        Instr::D { op: Op::ReadInt, src } => {
            let sz = b.ins().iconst(I64, 8);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let cm = b.ins().call(malloc, &[sz]);
            let buf = b.inst_results(cm)[0];
            let gv = str_gv(module, b, b"%lld\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let scanf = decl_rt(module, b.func, "scanf", &[I64, I64], &[I64])?;
            b.ins().call(scanf, &[fmt, buf]);
            let v = b.ins().load(I64, MemFlags::new(), buf, 0);
            let free = decl_rt(module, b.func, "free", &[I64], &[])?;
            b.ins().call(free, &[buf]);
            rs!(b, regs, *src, v);
        }
        Instr::D { op: Op::ReadFloat, src } => {
            let sz = b.ins().iconst(I64, 8);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let cm = b.ins().call(malloc, &[sz]);
            let buf = b.inst_results(cm)[0];
            let gv = str_gv(module, b, b"%lf\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let scanf = decl_rt(module, b.func, "scanf", &[I64, I64], &[I64])?;
            b.ins().call(scanf, &[fmt, buf]);
            let v = b.ins().load(I64, MemFlags::new(), buf, 0);
            let free = decl_rt(module, b.func, "free", &[I64], &[])?;
            b.ins().call(free, &[buf]);
            rs!(b, regs, *src, v);
        }
        Instr::D { op: Op::ReadLine, src } => {
            let sz = b.ins().iconst(I64, 4096);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let cm = b.ins().call(malloc, &[sz]);
            let buf = b.inst_results(cm)[0];
            let stdin_fn = decl_rt(module, b.func, "delta_get_stdin", &[], &[I64])?;
            let cs = b.ins().call(stdin_fn, &[]);
            let stdin = b.inst_results(cs)[0];
            let fgets = decl_rt(module, b.func, "fgets", &[I64, I64, I64], &[I64])?;
            b.ins().call(fgets, &[buf, sz, stdin]);
            rs!(b, regs, *src, buf);
        }

        // --- arrays: [len:i64][e0..eN-1:i64] ---
        Instr::B { op: Op::ArrNew, dst, imm } => {
            let total = b.ins().iconst(I64, 8 + *imm as i64 * 8);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let call = b.ins().call(malloc, &[total]);
            let ptr = b.inst_results(call)[0];
            let len = b.ins().iconst(I64, *imm as i64);
            b.ins().store(MemFlags::new(), len, ptr, 0);
            rs!(b, regs, *dst, ptr);
        }
        Instr::A { op: Op::ArrNewReg, dst, a, .. } => {
            let len = rg!(b, regs, *a);
            let eight = b.ins().iconst(I64, 8);
            let payload = b.ins().imul(len, eight);
            let total = b.ins().iadd(payload, eight);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let call = b.ins().call(malloc, &[total]);
            let ptr = b.inst_results(call)[0];
            b.ins().store(MemFlags::new(), len, ptr, 0);
            rs!(b, regs, *dst, ptr);
        }
        Instr::A { op: Op::ArrGet, dst, a, b: rb } => {
            let ptr = rg!(b, regs, *a);
            let idx = rg!(b, regs, *rb);
            let eight = b.ins().iconst(I64, 8);
            let off = b.ins().imul(idx, eight);
            let elem = b.ins().iadd(ptr, off);
            let v = b.ins().load(I64, MemFlags::new(), elem, 8);
            rs!(b, regs, *dst, v);
        }
        Instr::A { op: Op::ArrSet, dst, a, b: rb } => {
            let ptr = rg!(b, regs, *dst);
            let idx = rg!(b, regs, *a);
            let val = rg!(b, regs, *rb);
            let eight = b.ins().iconst(I64, 8);
            let off = b.ins().imul(idx, eight);
            let elem = b.ins().iadd(ptr, off);
            b.ins().store(MemFlags::new(), val, elem, 8);
        }
        Instr::A { op: Op::ArrLen, dst, a, .. } => {
            let ptr = rg!(b, regs, *a);
            let v = b.ins().load(I64, MemFlags::new(), ptr, 0);
            rs!(b, regs, *dst, v);
        }
        Instr::D { op: Op::ArrFree, src } => {
            let ptr = rg!(b, regs, *src);
            let f = decl_rt(module, b.func, "free", &[I64], &[])?;
            b.ins().call(f, &[ptr]);
        }

        // --- function pointers ---
        Instr::B { op: Op::FuncPtr, dst, imm } => {
            let fref = module.declare_func_in_func(func_ids[*imm as usize], b.func);
            let v = b.ins().func_addr(I64, fref);
            rs!(b, regs, *dst, v);
        }

        // --- time ---
        Instr::D { op: Op::TimeNs, src } |
        Instr::D { op: Op::TimeMs, src } |
        Instr::D { op: Op::TimeMonoNs, src } => {
            let clock_id = match instr {
                Instr::D { op: Op::TimeMonoNs, .. } => 1i64,
                _ => 0i64,
            };
            let ts_sz = b.ins().iconst(I64, 16);
            let malloc = decl_rt(module, b.func, "malloc", &[I64], &[I64])?;
            let cm = b.ins().call(malloc, &[ts_sz]);
            let ts = b.inst_results(cm)[0];
            let clk = b.ins().iconst(I64, clock_id);
            let cgt = decl_rt(module, b.func, "clock_gettime", &[I64, I64], &[I64])?;
            b.ins().call(cgt, &[clk, ts]);
            let secs = b.ins().load(I64, MemFlags::new(), ts, 0);
            let nsec = b.ins().load(I64, MemFlags::new(), ts, 8);
            let bn = b.ins().iconst(I64, 1_000_000_000);
            let mul = b.ins().imul(secs, bn);
            let total = b.ins().iadd(mul, nsec);
            let r = match instr {
                Instr::D { op: Op::TimeMs, .. } => {
                    let mn = b.ins().iconst(I64, 1_000_000);
                    b.ins().sdiv(total, mn)
                }
                _ => total,
            };
            let free = decl_rt(module, b.func, "free", &[I64], &[])?;
            b.ins().call(free, &[ts]);
            rs!(b, regs, *src, r);
        }

        // --- panic (terminator) ---
        Instr::D { op: Op::Panic, src } => {
            let ptr = rg!(b, regs, *src);
            let gv = str_gv(module, b, b"panic: %s\n\0")?;
            let fmt = b.ins().global_value(I64, gv);
            let printf = decl_rt(module, b.func, "printf", &[I64, I64], &[I64])?;
            b.ins().call(printf, &[fmt, ptr]);
            let one = b.ins().iconst(I64, 1);
            let exit = decl_rt(module, b.func, "exit", &[I64], &[])?;
            b.ins().call(exit, &[one]);
            b.ins().trap(TrapCode::UnreachableCodeReached);
            return Ok(true);
        }

        _ => {}
    }

    Ok(false)
}
