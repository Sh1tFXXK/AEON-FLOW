use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Inst {
    LoadImm { dst: u8, val: u64 },
    Mov { dst: u8, src: u8 },
    Add { dst: u8, a: u8, b: u8 },
    Sub { dst: u8, a: u8, b: u8 },
    Mul { dst: u8, a: u8, b: u8 },
    Jz { cond: u8, off: isize },
    Jump { offset: isize },
    Call { addr: usize },
    Ret,
    Print { r: u8 },
    LoadMem { dst: u8, addr: u8 },
    StoreMem { addr: u8, src: u8 },
    Alloc { dst: u8, size: u8 },
    Halt,
    Syscall { num: u8 },
}

impl Inst {
    pub fn disassemble(&self) -> String {
        match self {
            Inst::LoadImm { dst, val } => format!("load r{}, {}", dst, val),
            Inst::Mov { dst, src } => format!("mov r{}, r{}", dst, src),
            Inst::Add { dst, a, b } => format!("add r{}, r{}, r{}", dst, a, b),
            Inst::Sub { dst, a, b } => format!("sub r{}, r{}, r{}", dst, a, b),
            Inst::Mul { dst, a, b } => format!("mul r{}, r{}, r{}", dst, a, b),
            Inst::Jz { cond, off } => format!("jz r{}, {:+}", cond, off),
            Inst::Jump { offset } => format!("jump {:+}", offset),
            Inst::Call { addr } => format!("call {}", addr),
            Inst::Ret => "ret".to_string(),
            Inst::Print { r } => format!("print r{}", r),
            Inst::LoadMem { dst, addr } => format!("loadmem r{}, r{}", dst, addr),
            Inst::StoreMem { addr, src } => format!("storemem r{}, r{}", addr, src),
            Inst::Alloc { dst, size } => format!("alloc r{}, r{}", dst, size),
            Inst::Halt => "halt".to_string(),
            Inst::Syscall { num } => format!("syscall {}", num),
        }
    }
}
