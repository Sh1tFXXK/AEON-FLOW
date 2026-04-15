use serde::{Serialize, Deserialize};

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
    Halt,
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
            Inst::Halt => "halt".to_string(),
        }
    }
}
