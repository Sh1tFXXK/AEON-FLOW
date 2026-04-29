use crate::ProgramId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AeonEvent {
    Checkpoint {
        program_id: ProgramId,
        pc: usize,
        steps: usize,
    },
    VMMigrated {
        program_id: ProgramId,
        from: String,
        to: String,
        steps: usize,
    },
    PatchApplied {
        context_id: String,
        author: String,
        description: String,
        patch_count: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub event: AeonEvent,
    pub timestamp_ms: u64,
    pub prev_hash: [u8; 32],
    pub self_hash: [u8; 32],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventLog {
    entries: Vec<LogEntry>,
}

impl EventLog {
    pub fn append(&mut self, event: AeonEvent) -> [u8; 32] {
        self.append_at(event, now_ms())
    }

    pub fn append_at(&mut self, event: AeonEvent, timestamp_ms: u64) -> [u8; 32] {
        let prev_hash = self
            .entries
            .last()
            .map(|entry| entry.self_hash)
            .unwrap_or([0; 32]);
        let self_hash = hash_entry(&event, timestamp_ms, &prev_hash);
        self.entries.push(LogEntry {
            event,
            timestamp_ms,
            prev_hash,
            self_hash,
        });
        self_hash
    }

    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    pub fn entries_mut(&mut self) -> &mut [LogEntry] {
        &mut self.entries
    }

    pub fn extend_verified(&mut self, entries: &[LogEntry]) -> Result<(), String> {
        for entry in entries {
            let expected_prev = self
                .entries
                .last()
                .map(|entry| entry.self_hash)
                .unwrap_or([0; 32]);
            if entry.prev_hash != expected_prev {
                return Err("event prev_hash does not extend local chain".into());
            }
            if hash_entry(&entry.event, entry.timestamp_ms, &entry.prev_hash) != entry.self_hash {
                return Err("event self_hash mismatch".into());
            }
            self.entries.push(entry.clone());
        }
        Ok(())
    }

    pub fn verify(&self) -> Result<(), String> {
        let mut prev_hash = [0; 32];
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.prev_hash != prev_hash {
                return Err(format!("event {} prev_hash mismatch", index));
            }
            let expected = hash_entry(&entry.event, entry.timestamp_ms, &entry.prev_hash);
            if entry.self_hash != expected {
                return Err(format!("event {} self_hash mismatch", index));
            }
            prev_hash = entry.self_hash;
        }
        Ok(())
    }

    pub fn lines(&self) -> Vec<String> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                format!(
                    "{} {} prev={} self={} {:?}",
                    index,
                    entry.timestamp_ms,
                    short_hash(&entry.prev_hash),
                    short_hash(&entry.self_hash),
                    entry.event
                )
            })
            .collect()
    }
}

fn hash_entry(event: &AeonEvent, timestamp_ms: u64, prev_hash: &[u8; 32]) -> [u8; 32] {
    let payload = bincode::serialize(&(event, timestamp_ms, prev_hash)).unwrap();
    *blake3::hash(&payload).as_bytes()
}

fn short_hash(hash: &[u8; 32]) -> String {
    hash.iter()
        .take(4)
        .map(|byte| format!("{:02x}", byte))
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
