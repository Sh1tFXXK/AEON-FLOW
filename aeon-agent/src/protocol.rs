use aeon_store::{Platform, SignedBlob, CID};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMsg {
    Have {
        identity_id: [u8; 32],
        device_id: [u8; 16],
        cids: Vec<CID>,
        timestamp: u64,
    },
    Want { cid: CID },
    Data { blob: SignedBlob },
    Hello {
        identity_id: [u8; 32],
        device_id: [u8; 16],
        device_name: String,
        platform: Platform,
        public_key: Vec<u8>,
    },
    Ping { timestamp: u64 },
    Deleted {
        path: String,
        cid: CID,
        by: [u8; 32],
        at: u64,
        nonce: u64,
        public_key: Vec<u8>,
        signature: Vec<u8>,
    },
    CollabPatch {
        doc_id: CID,
        path: String,
        changes: Vec<u8>,
        by: [u8; 32],
        at: u64,
        nonce: u64,
        public_key: Vec<u8>,
        signature: Vec<u8>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SignedEnvelope {
    pub msg: SyncMsg,
    pub by: [u8; 32],
    pub at: u64,
    pub nonce: u64,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
}

impl SignedEnvelope {
    pub fn payload_bytes(msg:&SyncMsg, by:[u8;32], at:u64, nonce:u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&serde_json::to_vec(msg).unwrap_or_default());
        out.extend_from_slice(&by);
        out.extend_from_slice(&at.to_le_bytes());
        out.extend_from_slice(&nonce.to_le_bytes());
        out
    }
}
