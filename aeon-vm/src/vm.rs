use crate::cowheap::COWHeap;
use crate::inst::Inst;
use crate::program::Program;
use crate::vfs::VirtualFS;
use crate::EventLog;
use crate::ProgramId;
use serde::{Deserialize, Serialize};

pub const DEFAULT_HEAP_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub enum VMError {
    EmptyCallStack,
    MemoryOutOfBounds { addr: usize, heap_len: usize },
    OutOfMemory { requested: usize, available: usize },
    UnknownSyscall(u8),
    InvalidUtf8,
    Fs(String),
    RuntimeError(String),
}

impl std::fmt::Display for VMError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VMError::EmptyCallStack => write!(f, "empty call stack"),
            VMError::MemoryOutOfBounds { addr, heap_len } => {
                write!(
                    f,
                    "memory out of bounds: addr={} heap_len={}",
                    addr, heap_len
                )
            }
            VMError::OutOfMemory {
                requested,
                available,
            } => {
                write!(
                    f,
                    "out of memory: requested={} available={}",
                    requested, available
                )
            }
            VMError::UnknownSyscall(num) => write!(f, "unknown syscall: {}", num),
            VMError::InvalidUtf8 => write!(f, "invalid utf-8"),
            VMError::Fs(err) => write!(f, "filesystem error: {}", err),
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
    pub regs: Vec<u64>, // 256 registers
    pub pc: usize,
    pub call_stack: Vec<usize>,
    pub steps: usize,
    pub heap: COWHeap,
    pub heap_top: usize,
    pub vfs: VirtualFS,
    pub event_log: EventLog,
}

impl VMState {
    pub fn new(program: &Program) -> Self {
        VMState {
            program_id: program.id(),
            regs: vec![0; 256],
            pc: 0,
            call_stack: Vec::new(),
            steps: 0,
            heap: COWHeap::new(DEFAULT_HEAP_SIZE),
            heap_top: 0,
            vfs: VirtualFS::default(),
            event_log: EventLog::default(),
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
                self.regs[*dst as usize] =
                    self.regs[*a as usize].wrapping_add(self.regs[*b as usize]);
                self.pc += 1;
            }
            Inst::Sub { dst, a, b } => {
                self.regs[*dst as usize] =
                    self.regs[*a as usize].wrapping_sub(self.regs[*b as usize]);
                self.pc += 1;
            }
            Inst::Mul { dst, a, b } => {
                self.regs[*dst as usize] =
                    self.regs[*a as usize].wrapping_mul(self.regs[*b as usize]);
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
            Inst::LoadMem { dst, addr } => {
                let addr = match self.checked_heap_addr(*addr) {
                    Ok(addr) => addr,
                    Err(err) => return StepResult::Error(err),
                };
                self.regs[*dst as usize] = match self.heap.get(addr) {
                    Ok(byte) => byte as u64,
                    Err(_) => {
                        return StepResult::Error(VMError::MemoryOutOfBounds {
                            addr,
                            heap_len: self.heap.len(),
                        });
                    }
                };
                self.pc += 1;
            }
            Inst::StoreMem { addr, src } => {
                let addr = match self.checked_heap_addr(*addr) {
                    Ok(addr) => addr,
                    Err(err) => return StepResult::Error(err),
                };
                if self.heap.set(addr, self.regs[*src as usize] as u8).is_err() {
                    return StepResult::Error(VMError::MemoryOutOfBounds {
                        addr,
                        heap_len: self.heap.len(),
                    });
                }
                self.pc += 1;
            }
            Inst::Alloc { dst, size } => {
                let requested = match usize::try_from(self.regs[*size as usize]) {
                    Ok(size) => size,
                    Err(_) => {
                        return StepResult::Error(VMError::OutOfMemory {
                            requested: usize::MAX,
                            available: self.heap.len().saturating_sub(self.heap_top),
                        });
                    }
                };
                let Some(next_top) = self.heap_top.checked_add(requested) else {
                    return StepResult::Error(VMError::OutOfMemory {
                        requested,
                        available: self.heap.len().saturating_sub(self.heap_top),
                    });
                };
                if next_top > self.heap.len() {
                    return StepResult::Error(VMError::OutOfMemory {
                        requested,
                        available: self.heap.len().saturating_sub(self.heap_top),
                    });
                }
                self.regs[*dst as usize] = self.heap_top as u64;
                self.heap_top = next_top;
                self.pc += 1;
            }
            Inst::Syscall { num } => {
                if let Err(err) = self.handle_syscall(*num) {
                    return StepResult::Error(err);
                }
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

    fn checked_heap_addr(&self, reg: u8) -> Result<usize, VMError> {
        let addr =
            usize::try_from(self.regs[reg as usize]).map_err(|_| VMError::MemoryOutOfBounds {
                addr: usize::MAX,
                heap_len: self.heap.len(),
            })?;
        if addr >= self.heap.len() {
            return Err(VMError::MemoryOutOfBounds {
                addr,
                heap_len: self.heap.len(),
            });
        }
        Ok(addr)
    }

    fn handle_syscall(&mut self, num: u8) -> Result<(), VMError> {
        match num {
            0 => {
                let path_addr = self.reg_usize(1)?;
                let path_len = self.reg_usize(2)?;
                let writable = self.regs[3] != 0;
                let path_bytes = self.checked_heap_range(path_addr, path_len)?;
                let path = std::str::from_utf8(&path_bytes)
                    .map_err(|_| VMError::InvalidUtf8)?
                    .to_string();
                let fd = self
                    .vfs
                    .open(&path, writable)
                    .map_err(|err| VMError::Fs(err.to_string()))?;
                self.regs[0] = fd as u64;
                Ok(())
            }
            1 => {
                let fd = self.reg_usize(1)?;
                let buf_addr = self.reg_usize(2)?;
                let count = self.reg_usize(3)?;
                self.checked_heap_range(buf_addr, count)?;
                let mut buf = vec![0; count];
                let read = self
                    .vfs
                    .read(fd, &mut buf)
                    .map_err(|err| VMError::Fs(err.to_string()))?;
                self.heap.write(buf_addr, &buf[..read]).map_err(|_| {
                    VMError::MemoryOutOfBounds {
                        addr: buf_addr + read,
                        heap_len: self.heap.len(),
                    }
                })?;
                self.regs[0] = read as u64;
                Ok(())
            }
            2 => {
                let fd = self.reg_usize(1)?;
                let buf_addr = self.reg_usize(2)?;
                let count = self.reg_usize(3)?;
                let data = self.checked_heap_range(buf_addr, count)?;
                let written = self
                    .vfs
                    .write(fd, &data)
                    .map_err(|err| VMError::Fs(err.to_string()))?;
                self.regs[0] = written as u64;
                Ok(())
            }
            3 => {
                let fd = self.reg_usize(1)?;
                self.vfs
                    .close(fd)
                    .map_err(|err| VMError::Fs(err.to_string()))?;
                self.regs[0] = 0;
                Ok(())
            }
            other => Err(VMError::UnknownSyscall(other)),
        }
    }

    fn checked_heap_range(&self, addr: usize, len: usize) -> Result<Vec<u8>, VMError> {
        let Some(end) = addr.checked_add(len) else {
            return Err(VMError::MemoryOutOfBounds {
                addr,
                heap_len: self.heap.len(),
            });
        };
        if end > self.heap.len() {
            return Err(VMError::MemoryOutOfBounds {
                addr: end,
                heap_len: self.heap.len(),
            });
        }
        self.heap
            .read(addr, len)
            .map_err(|_| VMError::MemoryOutOfBounds {
                addr: end,
                heap_len: self.heap.len(),
            })
    }

    fn reg_usize(&self, reg: u8) -> Result<usize, VMError> {
        usize::try_from(self.regs[reg as usize]).map_err(|_| VMError::MemoryOutOfBounds {
            addr: usize::MAX,
            heap_len: self.heap.len(),
        })
    }
}
