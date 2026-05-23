use serde::{Deserialize, Serialize};

use crate::snapshot::Snapshot;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Patch {
    Reg {
        index: u8,
        old: u64,
        new: u64,
    },
    Pc {
        old: usize,
        new: usize,
    },
    CallStackEntry {
        index: usize,
        old: usize,
        new: usize,
    },
    HeapRange {
        addr: usize,
        old: Vec<u8>,
        new: Vec<u8>,
    },
    HeapTop {
        old: Option<usize>,
        new: Option<usize>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatchSet {
    pub description: String,
    pub patches: Vec<Patch>,
}

impl PatchSet {
    pub fn empty(description: impl Into<String>) -> Self {
        PatchSet {
            description: description.into(),
            patches: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.patches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    pub fn apply(&self, snap: &Snapshot) -> Result<Snapshot, String> {
        let mut next = snap.clone();

        for patch in &self.patches {
            match *patch {
                Patch::Reg { index, new, .. } => {
                    let slot = next
                        .regs
                        .get_mut(index as usize)
                        .ok_or_else(|| format!("register r{} out of range", index))?;
                    *slot = new;
                }
                Patch::Pc { new, .. } => next.pc = new,
                Patch::CallStackEntry { index, new, .. } => {
                    let slot = next
                        .call_stack
                        .get_mut(index)
                        .ok_or_else(|| format!("call stack index {} out of range", index))?;
                    *slot = new;
                }
                Patch::HeapRange { addr, ref new, .. } => {
                    let heap = next
                        .heap
                        .as_mut()
                        .ok_or_else(|| "heap is not available in this snapshot".to_string())?;
                    write_heap_range(heap, addr, new)?;
                }
                Patch::HeapTop { new, .. } => {
                    if let (Some(heap), Some(new)) = (next.heap.as_ref(), new) {
                        if new > heap.len() {
                            return Err(format!(
                                "heap_top {} exceeds heap length {}",
                                new,
                                heap.len()
                            ));
                        }
                    }
                    next.heap_top = new;
                }
            }
        }

        Ok(next)
    }

    pub fn reverse(&self) -> Self {
        let patches = self
            .patches
            .iter()
            .rev()
            .map(|patch| match patch {
                Patch::Reg { index, old, new } => Patch::Reg {
                    index: *index,
                    old: *new,
                    new: *old,
                },
                Patch::Pc { old, new } => Patch::Pc {
                    old: *new,
                    new: *old,
                },
                Patch::CallStackEntry { index, old, new } => Patch::CallStackEntry {
                    index: *index,
                    old: *new,
                    new: *old,
                },
                Patch::HeapRange { addr, old, new } => Patch::HeapRange {
                    addr: *addr,
                    old: new.clone(),
                    new: old.clone(),
                },
                Patch::HeapTop { old, new } => Patch::HeapTop {
                    old: *new,
                    new: *old,
                },
            })
            .collect();

        PatchSet {
            description: format!("undo: {}", self.description),
            patches,
        }
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        bincode::deserialize(data).map_err(|err| err.to_string())
    }
}

pub struct SnapshotEditor {
    working: Snapshot,
    description: String,
    patches: Vec<Patch>,
}

impl SnapshotEditor {
    pub fn new(snap: &Snapshot, description: impl Into<String>) -> Self {
        SnapshotEditor {
            working: snap.clone(),
            description: description.into(),
            patches: Vec::new(),
        }
    }

    pub fn set_reg(mut self, index: u8, val: u64) -> Result<Self, String> {
        let slot = self
            .working
            .regs
            .get_mut(index as usize)
            .ok_or_else(|| format!("register r{} out of range", index))?;
        let old = *slot;
        *slot = val;
        self.patches.push(Patch::Reg {
            index,
            old,
            new: val,
        });
        Ok(self)
    }

    pub fn set_pc(mut self, val: usize) -> Result<Self, String> {
        let old = self.working.pc;
        self.working.pc = val;
        self.patches.push(Patch::Pc { old, new: val });
        Ok(self)
    }

    pub fn set_call_stack_entry(mut self, index: usize, val: usize) -> Result<Self, String> {
        let slot = self
            .working
            .call_stack
            .get_mut(index)
            .ok_or_else(|| format!("call stack index {} out of range", index))?;
        let old = *slot;
        *slot = val;
        self.patches.push(Patch::CallStackEntry {
            index,
            old,
            new: val,
        });
        Ok(self)
    }

    pub fn set_heap_byte(self, addr: usize, val: u8) -> Result<Self, String> {
        self.set_heap_range(addr, vec![val])
    }

    pub fn set_heap_range(mut self, addr: usize, bytes: Vec<u8>) -> Result<Self, String> {
        let heap = self
            .working
            .heap
            .as_mut()
            .ok_or_else(|| "heap is not available in this snapshot".to_string())?;
        let old = read_heap_range(heap, addr, bytes.len())?;
        write_heap_range(heap, addr, &bytes)?;
        self.patches.push(Patch::HeapRange {
            addr,
            old,
            new: bytes,
        });
        Ok(self)
    }

    pub fn set_heap_str(self, addr: usize, text: &str) -> Result<Self, String> {
        self.set_heap_range(addr, text.as_bytes().to_vec())
    }

    pub fn set_heap_u64(self, addr: usize, val: u64) -> Result<Self, String> {
        self.set_heap_range(addr, val.to_le_bytes().to_vec())
    }

    pub fn set_heap_top(mut self, val: usize) -> Result<Self, String> {
        let heap = self
            .working
            .heap
            .as_ref()
            .ok_or_else(|| "heap is not available in this snapshot".to_string())?;
        if val > heap.len() {
            return Err(format!(
                "heap_top {} exceeds heap length {}",
                val,
                heap.len()
            ));
        }

        let old = self.working.heap_top;
        self.working.heap_top = Some(val);
        self.patches.push(Patch::HeapTop {
            old,
            new: Some(val),
        });
        Ok(self)
    }

    pub fn build(self) -> PatchSet {
        PatchSet {
            description: self.description,
            patches: self.patches,
        }
    }
}

pub struct Inspector<'a> {
    snap: &'a Snapshot,
}

impl<'a> Inspector<'a> {
    pub fn new(snap: &'a Snapshot) -> Self {
        Inspector { snap }
    }

    pub fn summary(&self) {
        println!(
            "program={:02x}{:02x}{:02x}{:02x} pc={} steps={} regs={}",
            self.snap.program_id[0],
            self.snap.program_id[1],
            self.snap.program_id[2],
            self.snap.program_id[3],
            self.snap.pc,
            self.snap.steps,
            self.snap.regs.len()
        );
    }

    pub fn dump_regs(&self, start: u8, end: u8) {
        let start = start as usize;
        let end = (end as usize).min(self.snap.regs.len().saturating_sub(1));
        for index in start..=end {
            println!("r{} = {}", index, self.snap.regs[index]);
        }
    }

    pub fn dump_heap(&self, addr: usize, len: usize) {
        let Some(heap) = self.snap.heap.as_ref() else {
            println!("heap is not available in this snapshot");
            return;
        };
        if addr >= heap.len() {
            println!("heap address {} out of range (len={})", addr, heap.len());
            return;
        }

        let end = addr.saturating_add(len).min(heap.len());
        for (line, chunk) in heap[addr..end].chunks(16).enumerate() {
            print!("{:08x}:", addr + line * 16);
            for byte in chunk {
                print!(" {:02x}", byte);
            }
            println!();
        }
    }

    pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<String> {
        let mut lines = Vec::new();

        if before.pc != after.pc {
            lines.push(format!("pc: {} -> {}", before.pc, after.pc));
        }
        if before.steps != after.steps {
            lines.push(format!("steps: {} -> {}", before.steps, after.steps));
        }
        if before.call_stack != after.call_stack {
            lines.push(format!(
                "call_stack: {:?} -> {:?}",
                before.call_stack, after.call_stack
            ));
        }
        if before.heap_top != after.heap_top {
            lines.push(format!(
                "heap_top: {:?} -> {:?}",
                before.heap_top, after.heap_top
            ));
        }
        if before.heap != after.heap {
            lines.push(format!(
                "heap: {} changed bytes",
                changed_heap_byte_count(before.heap.as_deref(), after.heap.as_deref())
            ));
        }

        let max = before.regs.len().max(after.regs.len());
        for index in 0..max {
            let old = before.regs.get(index).copied().unwrap_or_default();
            let new = after.regs.get(index).copied().unwrap_or_default();
            if old != new {
                lines.push(format!("r{}: {} -> {}", index, old, new));
            }
        }

        lines
    }
}

fn read_heap_range(heap: &[u8], addr: usize, len: usize) -> Result<Vec<u8>, String> {
    let end = checked_heap_end(heap, addr, len)?;
    Ok(heap[addr..end].to_vec())
}

fn write_heap_range(heap: &mut [u8], addr: usize, bytes: &[u8]) -> Result<(), String> {
    let end = checked_heap_end(heap, addr, bytes.len())?;
    heap[addr..end].copy_from_slice(bytes);
    Ok(())
}

fn checked_heap_end(heap: &[u8], addr: usize, len: usize) -> Result<usize, String> {
    let end = addr
        .checked_add(len)
        .ok_or_else(|| format!("heap range overflows: addr={} len={}", addr, len))?;
    if end > heap.len() {
        return Err(format!(
            "heap range {}..{} exceeds {}",
            addr,
            end,
            heap.len()
        ));
    }
    Ok(end)
}

fn changed_heap_byte_count(before: Option<&[u8]>, after: Option<&[u8]>) -> usize {
    match (before, after) {
        (Some(before), Some(after)) => {
            let shared = before
                .iter()
                .zip(after.iter())
                .filter(|(left, right)| left != right)
                .count();
            shared + before.len().abs_diff(after.len())
        }
        (Some(before), None) => before.len(),
        (None, Some(after)) => after.len(),
        (None, None) => 0,
    }
}
