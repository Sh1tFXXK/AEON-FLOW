use serde::{Serialize, Deserialize};
use std::path::Path;
use crate::vm::VMState;
use crate::store::ProgramStore;
use crate::ProgramId;
use crate::Program;
use bincode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub program_id: ProgramId,
    pub regs: Vec<u64>,
    pub pc: usize,
    pub call_stack: Vec<usize>,
    pub steps: usize,
}

impl Snapshot {
    pub fn capture(vm: &VMState) -> Self {
        Snapshot {
            program_id: vm.program_id(),
            regs: vm.regs.clone(),
            pc: vm.pc,
            call_stack: vm.call_stack.clone(),
            steps: vm.steps,
        }
    }

    pub fn from_vm(vm: &VMState) -> Self {
        Self::capture(vm)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        // Expect our format: 32-byte blake3 hash followed by bincode(serialized Snapshot)
        if bytes.len() < 32 {
            return Err(Box::new(bincode::ErrorKind::Custom("snapshot too short".into())));
        }
        let (hash_bytes, payload) = bytes.split_at(32);
        let digest = blake3::hash(payload);
        let computed = digest.as_bytes();
        if hash_bytes != computed {
            return Err(Box::new(bincode::ErrorKind::Custom("snapshot checksum mismatch".into())));
        }
        bincode::deserialize(payload)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let payload = bincode::serialize(self).unwrap();
        let hash = blake3::hash(&payload).as_bytes().to_vec();
        let mut out = Vec::with_capacity(hash.len() + payload.len());
        out.extend_from_slice(&hash);
        out.extend_from_slice(&payload);
        out
    }

    pub fn byte_size(&self) -> usize {
        self.to_bytes().len()
    }

    pub fn restore(&self, store: &ProgramStore) -> Result<VMState, String> {
        if let Some(program) = store.get(&self.program_id) {
            let mut vm = VMState::new(&*program);
            vm.regs = self.regs.clone();
            vm.pc = self.pc;
            vm.call_stack = self.call_stack.clone();
            vm.steps = self.steps;
            Ok(vm)
        } else {
            Err("program not found in store".into())
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        std::fs::write(path, bincode::serialize(self).unwrap())
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        bincode::deserialize(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn program_id(&self) -> ProgramId {
        self.program_id
    }
}
