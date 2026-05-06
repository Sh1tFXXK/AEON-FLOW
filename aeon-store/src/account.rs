use crate::CID;
use serde::{Deserialize, Serialize};

pub type AccountId = CID;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Account {
    pub id: AccountId,
    pub display_name: String,
    pub public_key: [u8; 32],
}

impl Account {
    pub fn from_public_key(display_name: &str, public_key: [u8; 32]) -> Self {
        let id = *blake3::hash(&public_key).as_bytes();
        Self {
            id,
            display_name: display_name.to_string(),
            public_key,
        }
    }
}
