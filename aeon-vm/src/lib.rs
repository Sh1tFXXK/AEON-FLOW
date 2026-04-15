#![doc(test = false)]
//! AEON VM - A minimal VM with complete state migration

pub mod inst;
pub mod program;
pub mod vm;
pub mod snapshot;
pub mod asm;
pub mod store;

// Re-export core types
pub use inst::Inst;
pub use program::Program;
pub use vm::VMState;
pub use snapshot::Snapshot;
pub use asm::Assembler;
pub use store::ProgramStore;

pub type ProgramId = [u8; 32];
