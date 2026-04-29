use crate::eventlog::{EventLog, LogEntry};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const BLOOM_BITS: usize = 8192;
const BLOOM_HASHES: u8 = 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EventGSet {
    pub known: HashSet<[u8; 32]>,
}

impl EventGSet {
    pub fn insert(&mut self, hash: [u8; 32]) {
        self.known.insert(hash);
    }

    pub fn merge(&mut self, other: &EventGSet) {
        self.known.extend(other.known.iter().copied());
    }

    pub fn from_log(log: &EventLog) -> Self {
        let mut set = EventGSet::default();
        for entry in log.entries() {
            set.insert(entry.self_hash);
        }
        set
    }

    pub fn from_entries(entries: &[LogEntry]) -> Self {
        let mut set = EventGSet::default();
        for entry in entries {
            set.insert(entry.self_hash);
        }
        set
    }

    pub fn len(&self) -> usize {
        self.known.len()
    }

    pub fn is_empty(&self) -> bool {
        self.known.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBloom {
    bits: Vec<u8>,
    bit_count: usize,
}

impl EventBloom {
    pub fn from_gset(set: &EventGSet) -> Self {
        let mut bloom = EventBloom {
            bits: vec![0; BLOOM_BITS / 8],
            bit_count: BLOOM_BITS,
        };
        for hash in &set.known {
            bloom.insert(hash);
        }
        bloom
    }

    pub fn insert(&mut self, hash: &[u8; 32]) {
        for seed in 0..BLOOM_HASHES {
            let bit = bloom_index(hash, seed, self.bit_count);
            self.bits[bit / 8] |= 1 << (bit % 8);
        }
    }

    pub fn might_contain(&self, hash: &[u8; 32]) -> bool {
        (0..BLOOM_HASHES).all(|seed| {
            let bit = bloom_index(hash, seed, self.bit_count);
            (self.bits[bit / 8] & (1 << (bit % 8))) != 0
        })
    }
}

pub fn missing_events(local_log: &EventLog, remote_bloom: &EventBloom) -> Vec<LogEntry> {
    local_log
        .entries()
        .iter()
        .filter(|entry| !remote_bloom.might_contain(&entry.self_hash))
        .cloned()
        .collect()
}

fn bloom_index(hash: &[u8; 32], seed: u8, bit_count: usize) -> usize {
    let mut input = [0u8; 33];
    input[0] = seed;
    input[1..].copy_from_slice(hash);
    let digest = blake3::hash(&input);
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(bytes) as usize % bit_count
}
