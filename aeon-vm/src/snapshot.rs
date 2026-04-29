use crate::store::ProgramStore;
use crate::vm::{VMState, DEFAULT_HEAP_SIZE};
use crate::ProgramId;
use bincode;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub format_version: u32,
    pub program_id: ProgramId,
    pub regs: Vec<u64>,
    pub pc: usize,
    pub call_stack: Vec<usize>,
    pub steps: usize,
    pub heap: Option<Vec<u8>>,
    pub heap_top: Option<usize>,
}

impl Snapshot {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn capture(vm: &VMState) -> Self {
        Snapshot {
            format_version: Self::CURRENT_VERSION,
            program_id: vm.program_id(),
            regs: vm.regs.clone(),
            pc: vm.pc,
            call_stack: vm.call_stack.clone(),
            steps: vm.steps,
            heap: Some(vm.heap.clone()),
            heap_top: Some(vm.heap_top),
        }
    }

    pub fn from_vm(vm: &VMState) -> Self {
        Self::capture(vm)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        if bytes.len() < 32 {
            return Err(bincode_error("snapshot missing checksum"));
        }

        let (hash_bytes, payload) = bytes.split_at(32);
        if hash_bytes != blake3::hash(payload).as_bytes() {
            return Err(bincode_error("snapshot checksum mismatch"));
        }

        let snap: Snapshot = bincode::deserialize(payload)?;
        if snap.format_version != Self::CURRENT_VERSION {
            return Err(bincode_error(format!(
                "unsupported snapshot format version {} (current {})",
                snap.format_version,
                Self::CURRENT_VERSION
            )));
        }

        Ok(snap)
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
            vm.heap = self.heap.clone().unwrap_or_else(default_heap);
            vm.heap_top = self.heap_top.unwrap_or(0);
            if vm.heap_top > vm.heap.len() {
                return Err(format!(
                    "heap_top {} exceeds heap length {}",
                    vm.heap_top,
                    vm.heap.len()
                ));
            }
            Ok(vm)
        } else {
            Err("program not found in store".into())
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        // Use the same checksum+payload format as to_bytes()
        std::fs::write(path, self.to_bytes())
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        // Use the same checksum+payload format as from_bytes()
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn program_id(&self) -> ProgramId {
        self.program_id
    }
}

fn default_heap() -> Vec<u8> {
    vec![0; DEFAULT_HEAP_SIZE]
}

fn bincode_error(message: impl Into<String>) -> bincode::Error {
    Box::new(bincode::ErrorKind::Custom(message.into()))
}
