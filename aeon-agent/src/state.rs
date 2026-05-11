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
pub struct CollabMetrics {
    pub applied_patches: u64,
    pub compacted_snapshots: u64,
}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileRecord {
    pub path: String,
    pub cid: String,
    pub identity_id: String,
    pub device_id: String,
    pub mime: String,
    pub observed_at: u64,
}


#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TunnelStatus {
    pub provider: String,
    pub endpoint: String,
    pub healthy: bool,
    pub state: String,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncState {
    pub seen_cids: HashSet<String>,
    pub tombstones: Vec<Tombstone>,
    pub collab_sessions: HashMap<String, String>,
    pub seen_nonces: HashMap<String, u64>,
    pub collab_metrics: HashMap<String, CollabMetrics>,
    pub trusted_keys: HashMap<String, String>,
    pub file_records: HashMap<String, FileRecord>,
    pub tunnel_status: Option<TunnelStatus>,
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

    pub fn nonce_seen(&self, by:[u8;32], nonce:u64) -> bool {
        self.seen_nonces.get(&format!("{}:{}", hex_cid(&by), nonce)).is_some()
    }

    pub fn mark_nonce(&mut self, by:[u8;32], nonce:u64, now:u64) {
        self.seen_nonces.insert(format!("{}:{}", hex_cid(&by), nonce), now);
    }

    pub fn cleanup_nonces(&mut self, now:u64, ttl:u64) {
        self.seen_nonces.retain(|_, ts| *ts + ttl >= now);
    }

    pub fn metric_mut(&mut self, doc_id_hex: &str) -> &mut CollabMetrics {
        self.collab_metrics.entry(doc_id_hex.to_string()).or_default()
    }

    pub fn trust_key(&mut self, identity_id: [u8; 32], public_key: &[u8]) -> bool {
        let id_hex = hex_cid(&identity_id);
        let pk_hex = hex::encode(public_key);
        match self.trusted_keys.get(&id_hex) {
            Some(existing) => existing == &pk_hex,
            None => {
                self.trusted_keys.insert(id_hex, pk_hex);
                true
            }
        }
    }

    pub fn trusted_key_for(&self, identity_id: [u8; 32]) -> Option<Vec<u8>> {
        self.trusted_keys
            .get(&hex_cid(&identity_id))
            .and_then(|hex| hex::decode(hex).ok())
    }

    pub fn record_file_ingest(
        &mut self,
        path: &str,
        cid: CID,
        identity_id: [u8; 32],
        device_id: [u8; 16],
        mime: &str,
        observed_at: u64,
    ) {
        self.file_records.insert(path.to_string(), FileRecord {
            path: path.to_string(),
            cid: hex_cid(&cid),
            identity_id: hex_cid(&identity_id),
            device_id: hex::encode(device_id),
            mime: mime.to_string(),
            observed_at,
        });
    }

    pub fn remove_file_record(&mut self, path: &str) {
        self.file_records.remove(path);
    }

    pub fn set_tunnel_status(&mut self, provider: String, endpoint: String, healthy: bool, state: String) {
        let updated_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        self.tunnel_status = Some(TunnelStatus { provider, endpoint, healthy, state, updated_at });
    }
}



