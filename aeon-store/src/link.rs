use crate::CID;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Link {
    pub target_cid: CID,
    pub label: String,
}

impl Link {
    pub fn new(target_cid: CID, label: &str) -> Self {
        Self {
            target_cid,
            label: label.to_string(),
        }
    }
}
