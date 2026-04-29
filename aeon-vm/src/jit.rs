use crate::inst::Inst;
use crate::program::Program;
use crate::vm::VMState;
use crate::ProgramId;
use cranelift::prelude::*;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{default_libcall_names, Linkage, Module, ModuleError};
use std::collections::{BTreeSet, HashMap};

pub const HOT_THRESHOLD: usize = 1_000;

type JitFn = unsafe extern "C" fn(*mut u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JitError {
    Unsupported(String),
    Compile(String),
}

impl std::fmt::Display for JitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JitError::Unsupported(msg) => write!(f, "unsupported JIT program: {}", msg),
            JitError::Compile(msg) => write!(f, "JIT compile error: {}", msg),
        }
    }
}

impl std::error::Error for JitError {}

impl From<ModuleError> for JitError {
    fn from(err: ModuleError) -> Self {
        JitError::Compile(err.to_string())
    }
}

#[derive(Clone, Copy)]
struct CompiledProgram {
    entry: JitFn,
}

pub struct JitEngine {
    counts: HashMap<ProgramId, usize>,
    compiled: HashMap<ProgramId, CompiledProgram>,
    module: JITModule,
    next_name: usize,
}

impl JitEngine {
    pub fn new() -> Result<Self, JitError> {
        let mut flag_builder = cranelift::codegen::settings::builder();
        flag_builder
            .set("opt_level", "speed")
            .map_err(|err| JitError::Compile(err.to_string()))?;
        let isa = cranelift_native::builder()
            .map_err(|err| JitError::Compile(err.to_string()))?
            .finish(cranelift::codegen::settings::Flags::new(flag_builder))
            .map_err(|err| JitError::Compile(err.to_string()))?;
        let builder = JITBuilder::with_isa(isa, default_libcall_names());
        Ok(Self {
            counts: HashMap::new(),
            compiled: HashMap::new(),
            module: JITModule::new(builder),
            next_name: 0,
        })
    }

    pub fn hot_count(&self, program: &Program) -> usize {
        self.counts.get(&program.id()).copied().unwrap_or(0)
    }

    pub fn has_compiled(&self, program: &Program) -> bool {
        self.compiled.contains_key(&program.id())
    }

    pub fn run_hot(&mut self, program: &Program, state: &mut VMState) -> Result<bool, JitError> {
        let id = program.id();
        let count = self.counts.entry(id).or_insert(0);
        *count += 1;
        if *count <= HOT_THRESHOLD {
            return Ok(false);
        }

        if !self.compiled.contains_key(&id) {
            match self.compile(program) {
                Ok(()) => {}
                Err(JitError::Unsupported(_)) => return Ok(false),
                Err(err) => return Err(err),
            }
        }
        self.run_compiled(program, state)?;
        Ok(true)
    }

    pub fn compile(&mut self, program: &Program) -> Result<(), JitError> {
        let id = program.id();
        if self.compiled.contains_key(&id) {
            return Ok(());
        }

        validate_program(program)?;

        let name = format!("aeon_jit_{}", self.next_name);
        self.next_name += 1;

        let mut ctx = self.module.make_context();
        let mut builder_ctx = FunctionBuilderContext::new();
        let ptr_ty = self.module.target_config().pointer_type();
        ctx.func.signature.params.push(AbiParam::new(ptr_ty));

        build_function(&mut ctx.func, &mut builder_ctx, ptr_ty, program)?;

        let func_id = self
            .module
            .declare_function(&name, Linkage::Local, &ctx.func.signature)?;
        self.module.define_function(func_id, &mut ctx)?;
        self.module.clear_context(&mut ctx);
        self.module.finalize_definitions()?;

        let code = self.module.get_finalized_function(func_id);
        let entry = unsafe { std::mem::transmute::<*const u8, JitFn>(code) };
        self.compiled.insert(id, CompiledProgram { entry });
        Ok(())
    }

    pub fn run_compiled(&mut self, program: &Program, state: &mut VMState) -> Result<(), JitError> {
        let id = program.id();
        if state.pc != 0 {
            return Err(JitError::Unsupported(
                "compiled entry currently requires pc=0".to_string(),
            ));
        }
        if !self.compiled.contains_key(&id) {
            self.compile(program)?;
        }
        self.run_compiled_cached(id, program.instructions.len(), state)
    }

    pub fn run_compiled_cached(
        &self,
        program_id: ProgramId,
        program_len: usize,
        state: &mut VMState,
    ) -> Result<(), JitError> {
        if state.pc != 0 {
            return Err(JitError::Unsupported(
                "compiled entry currently requires pc=0".to_string(),
            ));
        }
        let compiled = self.compiled.get(&program_id).ok_or_else(|| {
            JitError::Unsupported("program has not been compiled yet".to_string())
        })?;
        unsafe {
            (compiled.entry)(state.regs.as_mut_ptr());
        }
        state.pc = program_len;
        Ok(())
    }
}

fn validate_program(program: &Program) -> Result<(), JitError> {
    for (pc, inst) in program.instructions.iter().enumerate() {
        match inst {
            Inst::LoadImm { .. }
            | Inst::Mov { .. }
            | Inst::Add { .. }
            | Inst::Sub { .. }
            | Inst::Mul { .. }
            | Inst::Halt => {}
            Inst::Jz { off, .. } => {
                checked_target(pc, *off, program.instructions.len())?;
            }
            Inst::Jump { offset } => {
                checked_target(pc, *offset, program.instructions.len())?;
            }
            other => {
                return Err(JitError::Unsupported(format!(
                    "{} has side effects or VM state not modeled yet",
                    other.disassemble()
                )));
            }
        }
    }
    Ok(())
}

fn build_function(
    func: &mut cranelift::codegen::ir::Function,
    builder_ctx: &mut FunctionBuilderContext,
    ptr_ty: Type,
    program: &Program,
) -> Result<(), JitError> {
    let mut builder = FunctionBuilder::new(func, builder_ctx);
    let leaders = basic_block_leaders(program)?;
    let mut blocks = HashMap::new();
    for leader in &leaders {
        blocks.insert(*leader, builder.create_block());
    }

    let regs_var = Variable::new(0);
    builder.declare_var(regs_var, ptr_ty);
    let entry_block = blocks[&0];
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    let regs_ptr = builder.block_params(entry_block)[0];
    builder.def_var(regs_var, regs_ptr);

    let touched_regs = touched_registers(program);
    let mut reg_vars = HashMap::new();
    for (idx, reg) in touched_regs.iter().enumerate() {
        let var = Variable::new(idx + 1);
        builder.declare_var(var, types::I64);
        let value = load_reg(&mut builder, regs_ptr, *reg);
        builder.def_var(var, value);
        reg_vars.insert(*reg, var);
    }

    for window in leaders.windows(2) {
        let start = window[0];
        let end = window[1];
        if start == program.instructions.len() {
            continue;
        }
        if start != 0 {
            builder.switch_to_block(blocks[&start]);
        }

        let mut pc = start;
        let mut terminated = false;
        while pc < end {
            match &program.instructions[pc] {
                Inst::LoadImm { dst, val } => {
                    let value = builder.ins().iconst(types::I64, *val as i64);
                    store_var(&mut builder, &reg_vars, *dst, value);
                    pc += 1;
                }
                Inst::Mov { dst, src } => {
                    let value = load_var(&mut builder, &reg_vars, *src);
                    store_var(&mut builder, &reg_vars, *dst, value);
                    pc += 1;
                }
                Inst::Add { dst, a, b } => {
                    let lhs = load_var(&mut builder, &reg_vars, *a);
                    let rhs = load_var(&mut builder, &reg_vars, *b);
                    let value = builder.ins().iadd(lhs, rhs);
                    store_var(&mut builder, &reg_vars, *dst, value);
                    pc += 1;
                }
                Inst::Sub { dst, a, b } => {
                    let lhs = load_var(&mut builder, &reg_vars, *a);
                    let rhs = load_var(&mut builder, &reg_vars, *b);
                    let value = builder.ins().isub(lhs, rhs);
                    store_var(&mut builder, &reg_vars, *dst, value);
                    pc += 1;
                }
                Inst::Mul { dst, a, b } => {
                    let lhs = load_var(&mut builder, &reg_vars, *a);
                    let rhs = load_var(&mut builder, &reg_vars, *b);
                    let value = builder.ins().imul(lhs, rhs);
                    store_var(&mut builder, &reg_vars, *dst, value);
                    pc += 1;
                }
                Inst::Jz { cond, off } => {
                    let value = load_var(&mut builder, &reg_vars, *cond);
                    let zero = builder.ins().iconst(types::I64, 0);
                    let is_zero = builder.ins().icmp(IntCC::Equal, value, zero);
                    let target = checked_target(pc, *off, program.instructions.len())?;
                    builder
                        .ins()
                        .brif(is_zero, blocks[&target], &[], blocks[&(pc + 1)], &[]);
                    terminated = true;
                    break;
                }
                Inst::Jump { offset } => {
                    let target = checked_target(pc, *offset, program.instructions.len())?;
                    builder.ins().jump(blocks[&target], &[]);
                    terminated = true;
                    break;
                }
                Inst::Halt => {
                    builder.ins().jump(blocks[&program.instructions.len()], &[]);
                    terminated = true;
                    break;
                }
                _ => unreachable!("validate_program rejects unsupported instructions"),
            }
        }
        if !terminated {
            builder.ins().jump(blocks[&end], &[]);
        }
    }

    builder.switch_to_block(blocks[&program.instructions.len()]);
    let regs_ptr = builder.use_var(regs_var);
    for reg in touched_regs {
        let value = load_var(&mut builder, &reg_vars, reg);
        store_reg(&mut builder, regs_ptr, reg, value);
    }
    builder.ins().return_(&[]);
    builder.seal_all_blocks();
    builder.finalize();
    Ok(())
}

fn basic_block_leaders(program: &Program) -> Result<Vec<usize>, JitError> {
    let len = program.instructions.len();
    let mut leaders = BTreeSet::from([0, len]);
    for (pc, inst) in program.instructions.iter().enumerate() {
        match inst {
            Inst::Jz { off, .. } => {
                leaders.insert(checked_target(pc, *off, len)?);
                leaders.insert(pc + 1);
            }
            Inst::Jump { offset } => {
                leaders.insert(checked_target(pc, *offset, len)?);
                leaders.insert(pc + 1);
            }
            Inst::Halt => {
                leaders.insert(pc + 1);
            }
            _ => {}
        }
    }
    Ok(leaders
        .into_iter()
        .filter(|leader| *leader <= len)
        .collect())
}

fn touched_registers(program: &Program) -> Vec<u8> {
    let mut regs = BTreeSet::new();
    for inst in &program.instructions {
        match inst {
            Inst::LoadImm { dst, .. } => {
                regs.insert(*dst);
            }
            Inst::Mov { dst, src } => {
                regs.insert(*dst);
                regs.insert(*src);
            }
            Inst::Add { dst, a, b } | Inst::Sub { dst, a, b } | Inst::Mul { dst, a, b } => {
                regs.insert(*dst);
                regs.insert(*a);
                regs.insert(*b);
            }
            Inst::Jz { cond, .. } => {
                regs.insert(*cond);
            }
            Inst::Jump { .. } | Inst::Halt => {}
            _ => unreachable!("validate_program rejects unsupported instructions"),
        }
    }
    regs.into_iter().collect()
}

fn load_var(builder: &mut FunctionBuilder, reg_vars: &HashMap<u8, Variable>, reg: u8) -> Value {
    builder.use_var(reg_vars[&reg])
}

fn store_var(
    builder: &mut FunctionBuilder,
    reg_vars: &HashMap<u8, Variable>,
    reg: u8,
    value: Value,
) {
    builder.def_var(reg_vars[&reg], value);
}

fn checked_target(pc: usize, offset: isize, len: usize) -> Result<usize, JitError> {
    let target = pc as isize + offset;
    if target < 0 || target as usize > len {
        return Err(JitError::Unsupported(format!(
            "branch from pc={} to {} is outside 0..={}",
            pc, target, len
        )));
    }
    Ok(target as usize)
}

fn load_reg(builder: &mut FunctionBuilder, regs_ptr: Value, reg: u8) -> Value {
    builder.ins().load(
        types::I64,
        MemFlags::new(),
        regs_ptr,
        i32::from(reg) * std::mem::size_of::<u64>() as i32,
    )
}

fn store_reg(builder: &mut FunctionBuilder, regs_ptr: Value, reg: u8, value: Value) {
    builder.ins().store(
        MemFlags::new(),
        value,
        regs_ptr,
        i32::from(reg) * std::mem::size_of::<u64>() as i32,
    );
}
