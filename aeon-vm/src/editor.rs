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
            }
        }

        Ok(next)
    }

    pub fn reverse(&self) -> Self {
        let patches = self
            .patches
            .iter()
            .rev()
            .map(|patch| match *patch {
                Patch::Reg { index, old, new } => Patch::Reg {
                    index,
                    old: new,
                    new: old,
                },
                Patch::Pc { old, new } => Patch::Pc { old: new, new: old },
                Patch::CallStackEntry { index, old, new } => Patch::CallStackEntry {
                    index,
                    old: new,
                    new: old,
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

    pub fn set_heap_byte(self, _addr: usize, _val: u8) -> Result<Self, String> {
        Err("heap editing is deferred until Step 3".into())
    }

    pub fn set_heap_range(self, _addr: usize, _bytes: Vec<u8>) -> Result<Self, String> {
        Err("heap editing is deferred until Step 3".into())
    }

    pub fn set_heap_str(self, _addr: usize, _text: &str) -> Result<Self, String> {
        Err("heap editing is deferred until Step 3".into())
    }

    pub fn set_heap_u64(self, _addr: usize, _val: u64) -> Result<Self, String> {
        Err("heap editing is deferred until Step 3".into())
    }

    pub fn set_heap_top(self, _val: usize) -> Result<Self, String> {
        Err("heap editing is deferred until Step 3".into())
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
