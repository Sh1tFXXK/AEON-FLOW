//! AEON VM - A minimal VM with complete state migration

pub mod asm;
pub mod console;
pub mod cowheap;
pub mod daemon;
pub mod data_layer;
pub mod editor;
pub mod eventlog;
pub mod forth;
pub mod inst;
pub mod jit;
pub mod p2p;
pub mod program;
pub mod protocol;
pub mod session;
pub mod snapshot;
pub mod store;
pub mod vfs;
pub mod vm;

// Re-export core types
pub use asm::Assembler;
pub use data_layer::{store_patchset, store_vm_snapshot, StoredPatchData, StoredVMData};
pub use eventlog::{AeonEvent, EventLog};
pub use forth::ForthPrototype;
pub use inst::Inst;
pub use jit::JitEngine;
pub use program::Program;
pub use snapshot::Snapshot;
pub use store::ProgramStore;
pub use vfs::VirtualFS;
pub use vm::VMState;

pub type ProgramId = [u8; 32];
