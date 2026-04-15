use serde::{Serialize, Deserialize};
use crate::inst::Inst;
use crate::program::Program;
use crate::ProgramId;

#[derive(Debug, Clone, PartialEq)]
pub enum VMError {
    EmptyCallStack,
    RuntimeError(String),
}

impl std::fmt::Display for VMError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VMError::EmptyCallStack => write!(f, "empty call stack"),
            VMError::RuntimeError(s) => write!(f, "runtime error: {}", s),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum StepResult {
    Ok,
    Halted,
    Error(VMError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMState {
    program_id: ProgramId,
    pub regs: Vec<u64>,  // 256 registers
    pub pc: usize,
    pub call_stack: Vec<usize>,
    pub steps: usize,
}

impl VMState {
    pub fn new(program: &Program) -> Self {
        VMState {
            program_id: program.id(),
            regs: vec![0; 256],
            pc: 0,
            call_stack: Vec::new(),
            steps: 0,
        }
    }

    pub fn step(&mut self, program: &Program) -> StepResult {
        if self.pc >= program.instructions.len() {
            return StepResult::Halted;
        }

        match &program.instructions[self.pc] {
            Inst::LoadImm { dst, val } => {
                self.regs[*dst as usize] = *val;
                self.pc += 1;
            }
            Inst::Mov { dst, src } => {
                self.regs[*dst as usize] = self.regs[*src as usize];
                self.pc += 1;
            }
            Inst::Add { dst, a, b } => {
                self.regs[*dst as usize] = self.regs[*a as usize]
                    .wrapping_add(self.regs[*b as usize]);
                self.pc += 1;
            }
            Inst::Sub { dst, a, b } => {
                self.regs[*dst as usize] = self.regs[*a as usize]
                    .wrapping_sub(self.regs[*b as usize]);
                self.pc += 1;
            }
            Inst::Mul { dst, a, b } => {
                self.regs[*dst as usize] = self.regs[*a as usize]
                    .wrapping_mul(self.regs[*b as usize]);
                self.pc += 1;
            }
            Inst::Jz { cond, off } => {
                if self.regs[*cond as usize] == 0 {
                    self.pc = (self.pc as isize + *off) as usize;
                } else {
                    self.pc += 1;
                }
            }
            Inst::Jump { offset } => {
                self.pc = (self.pc as isize + *offset) as usize;
            }
            Inst::Call { addr } => {
                self.call_stack.push(self.pc + 1);
                self.pc = *addr;
            }
            Inst::Ret => {
                if let Some(ret_pc) = self.call_stack.pop() {
                    self.pc = ret_pc;
                } else {
                    return StepResult::Error(VMError::EmptyCallStack);
                }
            }
            Inst::Print { r } => {
                println!("r[{}] = {}", r, self.regs[*r as usize]);
                self.pc += 1;
            }
            Inst::Halt => return StepResult::Halted,
        }
        self.steps += 1;
        StepResult::Ok
    }

    pub fn run(&mut self, program: &Program) -> Result<usize, VMError> {
        loop {
            match self.step(program) {
                StepResult::Ok => continue,
                StepResult::Halted => return Ok(self.steps),
                StepResult::Error(e) => return Err(e),
            }
        }
    }

    pub fn run_bounded(&mut self, program: &Program, max_steps: usize) -> (usize, bool) {
        for _ in 0..max_steps {
            match self.step(program) {
                StepResult::Ok => continue,
                StepResult::Halted => return (self.steps, true),
                StepResult::Error(_) => return (self.steps, false),
            }
        }
        (self.steps, false)
    }

    pub fn program_id(&self) -> ProgramId {
        self.program_id
    }
}
