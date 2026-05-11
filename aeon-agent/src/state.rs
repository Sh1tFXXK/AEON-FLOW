use aeon_store::{hex_cid, parse_cid_hex, CID};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tombstone {
    pub path: String,
    pub cid: String,
    pub at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CollabMetrics {
    pub applied_patches: u64,
    pub compacted_snapshots: u64,
}

pub struct SyncState {
    pub seen_cids: HashSet<String>,
    pub tombstones: Vec<Tombstone>,
    pub collab_sessions: HashMap<String, String>,
    pub seen_nonces: HashMap<String, u64>,
    pub collab_metrics: HashMap<String, CollabMetrics>,
}

impl SyncState {
    pub fn load(path: &PathBuf) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &PathBuf) {
        if let Some(p) = path.parent() { let _ = std::fs::create_dir_all(p); }
        if let Ok(bytes) = serde_json::to_vec_pretty(self) { let _ = std::fs::write(path, bytes); }
    }

    pub fn has_seen(&self, cid: &CID) -> bool { self.seen_cids.contains(&hex_cid(cid)) }
    pub fn mark_seen(&mut self, cid: &CID) { self.seen_cids.insert(hex_cid(cid)); }

    pub fn add_tombstone(&mut self, path: String, cid: CID, at: u64) {
        self.tombstones.push(Tombstone { path, cid: hex_cid(&cid), at });
        if self.tombstones.len() > 10_000 { self.tombstones.drain(0..2000); }
    }

    pub fn collab_doc_for_path(&mut self, path: &str) -> CID {
        if let Some(hex) = self.collab_sessions.get(path) {
            if let Ok(cid) = parse_cid_hex(hex) {
                return cid;
            }
        }
        let cid = *blake3::hash(path.as_bytes()).as_bytes();
        self.collab_sessions.insert(path.to_string(), hex_cid(&cid));
        cid
    }

    pub fn tombstone_map(&self) -> HashMap<String, CID> {
        let mut m = HashMap::new();
        for t in &self.tombstones {
            if let Ok(cid) = parse_cid_hex(&t.cid) { m.insert(t.path.clone(), cid); }
        }
        m
    }
}

impl SyncState {
    pub fn nonce_seen(&self, by:[u8;32], nonce:u64) -> bool {
        self.seen_nonces.get(&format!("{}:{}", hex_cid(&by), nonce)).is_some()
    }
    pub fn mark_nonce(&mut self, by:[u8;32], nonce:u64, now:u64) {
        self.seen_nonces.insert(format!("{}:{}", hex_cid(&by), nonce), now);
    }
    pub fn cleanup_nonces(&mut self, now:u64, ttl:u64) {
        self.seen_nonces.retain(|_, ts| *ts + ttl >= now);
    }
}

impl SyncState {
    pub fn metric_mut(&mut self, doc_id_hex: &str) -> &mut CollabMetrics {
        self.collab_metrics.entry(doc_id_hex.to_string()).or_default()
    }
}
