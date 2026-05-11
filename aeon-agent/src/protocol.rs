use aeon_store::{Platform, SignedBlob, CID};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
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
    },
    Ping { timestamp: u64 },
    Deleted {
        path: String,
        cid: CID,
        by: [u8; 32],
        at: u64,
    },
}
