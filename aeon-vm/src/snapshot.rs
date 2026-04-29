use crate::cowheap::{COWHeap, HeapPageDelta};
use crate::store::ProgramStore;
use crate::vfs::VirtualFS;
use crate::vm::{VMState, DEFAULT_HEAP_SIZE};
use crate::{AeonEvent, EventLog, ProgramId};
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
    pub vfs: VirtualFS,
    pub event_log: EventLog,
}

impl Snapshot {
    pub const CURRENT_VERSION: u32 = 2;

    pub fn capture(vm: &VMState) -> Self {
        let mut event_log = vm.event_log.clone();
        event_log.append(AeonEvent::Checkpoint {
            program_id: vm.program_id(),
            pc: vm.pc,
            steps: vm.steps,
        });
        Snapshot {
            format_version: Self::CURRENT_VERSION,
            program_id: vm.program_id(),
            regs: vm.regs.clone(),
            pc: vm.pc,
            call_stack: vm.call_stack.clone(),
            steps: vm.steps,
            heap: Some(vm.heap.to_vec()),
            heap_top: Some(vm.heap_top),
            vfs: vm.vfs.clone(),
            event_log,
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
            vm.heap = COWHeap::from_bytes(&self.heap.clone().unwrap_or_else(default_heap));
            vm.heap_top = self.heap_top.unwrap_or(0);
            vm.vfs = self.vfs.clone();
            vm.event_log = self.event_log.clone();
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

    pub fn append_event(&mut self, event: AeonEvent) -> [u8; 32] {
        self.event_log.append(event)
    }
}

fn default_heap() -> Vec<u8> {
    vec![0; DEFAULT_HEAP_SIZE]
}

fn bincode_error(message: impl Into<String>) -> bincode::Error {
    Box::new(bincode::ErrorKind::Custom(message.into()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotDelta {
    pub format_version: u32,
    pub program_id: ProgramId,
    pub regs: Vec<u64>,
    pub pc: usize,
    pub call_stack: Vec<usize>,
    pub steps: usize,
    pub heap_len: usize,
    pub heap_top: usize,
    pub dirty_pages: Vec<HeapPageDelta>,
    pub vfs: VirtualFS,
    pub event_log: EventLog,
}

impl SnapshotDelta {
    pub fn capture(vm: &VMState) -> Self {
        let mut event_log = vm.event_log.clone();
        event_log.append(AeonEvent::Checkpoint {
            program_id: vm.program_id(),
            pc: vm.pc,
            steps: vm.steps,
        });
        SnapshotDelta {
            format_version: Snapshot::CURRENT_VERSION,
            program_id: vm.program_id(),
            regs: vm.regs.clone(),
            pc: vm.pc,
            call_stack: vm.call_stack.clone(),
            steps: vm.steps,
            heap_len: vm.heap.len(),
            heap_top: vm.heap_top,
            dirty_pages: vm.heap.dirty_pages(),
            vfs: vm.vfs.clone(),
            event_log,
        }
    }

    pub fn apply_to(&self, base: &Snapshot) -> Result<Snapshot, String> {
        if self.program_id != base.program_id {
            return Err("delta ProgramId does not match base snapshot".into());
        }

        let mut heap = COWHeap::from_bytes(base.heap.as_deref().unwrap_or(&[]));
        if heap.len() != self.heap_len {
            return Err(format!(
                "delta heap length {} does not match base {}",
                self.heap_len,
                heap.len()
            ));
        }
        heap.apply_pages(&self.dirty_pages)?;

        Ok(Snapshot {
            format_version: self.format_version,
            program_id: self.program_id,
            regs: self.regs.clone(),
            pc: self.pc,
            call_stack: self.call_stack.clone(),
            steps: self.steps,
            heap: Some(heap.to_vec()),
            heap_top: Some(self.heap_top),
            vfs: self.vfs.clone(),
            event_log: self.event_log.clone(),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, bincode::Error> {
        bincode::deserialize(bytes)
    }
}
