//! AEON VM - A minimal VM with complete state migration

pub mod asm;
pub mod console;
pub mod editor;
pub mod forth;
pub mod inst;
pub mod jit;
pub mod program;
pub mod protocol;
pub mod session;
pub mod snapshot;
pub mod store;
pub mod vfs;
pub mod vm;

// Re-export core types
pub use asm::Assembler;
pub use forth::ForthPrototype;
pub use inst::Inst;
pub use jit::JitEngine;
pub use program::Program;
pub use snapshot::Snapshot;
pub use store::ProgramStore;
pub use vfs::VirtualFS;
pub use vm::VMState;

pub type ProgramId = [u8; 32];
