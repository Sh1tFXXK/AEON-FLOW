use crate::inst::Inst;
use crate::ProgramId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramMetadata {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    pub metadata: ProgramMetadata,
    pub instructions: Vec<Inst>,
}

impl Program {
    pub fn new(instructions: Vec<Inst>) -> Self {
        Program {
            metadata: ProgramMetadata {
                name: "unnamed".into(),
            },
            instructions,
        }
    }

    pub fn from_parts(name: String, instructions: Vec<Inst>) -> Self {
        Program {
            metadata: ProgramMetadata { name },
            instructions,
        }
    }

    pub fn id(&self) -> ProgramId {
        let bytes = bincode::serialize(&self.instructions).unwrap();
        blake3::hash(&bytes).into()
    }

    pub fn instruction_count(&self) -> usize {
        self.instructions.len()
    }

    pub fn disassemble(&self) -> String {
        self.instructions
            .iter()
            .enumerate()
            .map(|(i, inst)| format!("{:04}: {}", i, inst.disassemble()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(data)
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        std::fs::write(path, self.to_bytes())
    }

    pub fn load(path: &std::path::Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

pub mod programs {
    use super::*;
    pub fn fibonacci(n: u64) -> Program {
        // Use n-1 as the loop counter so the canonical loop body executes
        // the correct number of times to produce fib(n).
        let counter = if n == 0 { 0 } else { n - 1 };
        Program::new(vec![
            Inst::LoadImm {
                dst: 0,
                val: counter,
            },
            Inst::LoadImm { dst: 1, val: 0 },
            Inst::LoadImm { dst: 2, val: 1 },
            Inst::LoadImm { dst: 4, val: 1 },
            Inst::Jz { cond: 0, off: 6 },
            Inst::Add { dst: 3, a: 1, b: 2 },
            Inst::Mov { dst: 1, src: 2 },
            Inst::Mov { dst: 2, src: 3 },
            Inst::Sub { dst: 0, a: 0, b: 4 },
            Inst::Jump { offset: -5 },
            Inst::Halt,
        ])
    }

    pub fn factorial(n: u64) -> Program {
        Program::new(vec![
            Inst::LoadImm { dst: 0, val: n },
            Inst::LoadImm { dst: 1, val: 1 },
            Inst::LoadImm { dst: 2, val: 1 },
            Inst::Mul { dst: 1, a: 1, b: 0 },
            Inst::Sub { dst: 0, a: 0, b: 2 },
            Inst::Jz { cond: 0, off: 2 },
            Inst::Jump { offset: -3 },
            Inst::Halt,
        ])
    }
}
