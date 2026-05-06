use crate::{hex_cid, AccountId, Blob, CID};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const CONTEXT_MIME: &str = "application/x-aeon-context";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    pub id: String,
    pub name: String,
    pub owner: AccountId,
    pub members: Vec<AccountId>,
    pub nodes: Vec<CID>,
    pub events: Vec<ContextEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextEvent {
    NodeAdded {
        node_cid: CID,
        by: AccountId,
        at: u64,
    },
    NodeEdited {
        old_cid: CID,
        new_cid: CID,
        by: AccountId,
        at: u64,
    },
    NodeRemoved {
        node_cid: CID,
        by: AccountId,
        at: u64,
    },
    MemberJoined {
        account: AccountId,
        at: u64,
    },
    Message {
        text: String,
        by: AccountId,
        at: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextError {
    NotMember(AccountId),
    MissingNode(CID),
}

impl Context {
    pub fn new(name: &str, owner: AccountId) -> Self {
        let at = current_millis();
        let id = context_id(name, &owner, at);
        Self {
            id,
            name: name.to_string(),
            owner,
            members: vec![owner],
            nodes: Vec::new(),
            events: vec![ContextEvent::MemberJoined { account: owner, at }],
        }
    }

    pub fn is_member(&self, account: &AccountId) -> bool {
        self.members.contains(account)
    }

    pub fn add_member(&mut self, account: AccountId, at: u64) {
        if !self.members.contains(&account) {
            self.members.push(account);
            self.events.push(ContextEvent::MemberJoined { account, at });
        }
    }

    pub fn add_node(&mut self, node_cid: CID, by: AccountId, at: u64) -> Result<(), ContextError> {
        self.require_member(by)?;
        if !self.nodes.contains(&node_cid) {
            self.nodes.push(node_cid);
        }
        self.events
            .push(ContextEvent::NodeAdded { node_cid, by, at });
        Ok(())
    }

    pub fn update_node(
        &mut self,
        old_cid: CID,
        new_cid: CID,
        by: AccountId,
        at: u64,
    ) -> Result<(), ContextError> {
        self.require_member(by)?;
        let Some(pos) = self.nodes.iter().position(|cid| *cid == old_cid) else {
            return Err(ContextError::MissingNode(old_cid));
        };
        self.nodes[pos] = new_cid;
        self.events.push(ContextEvent::NodeEdited {
            old_cid,
            new_cid,
            by,
            at,
        });
        Ok(())
    }

    pub fn remove_node(
        &mut self,
        node_cid: CID,
        by: AccountId,
        at: u64,
    ) -> Result<(), ContextError> {
        self.require_member(by)?;
        let Some(pos) = self.nodes.iter().position(|cid| *cid == node_cid) else {
            return Err(ContextError::MissingNode(node_cid));
        };
        self.nodes.remove(pos);
        self.events
            .push(ContextEvent::NodeRemoved { node_cid, by, at });
        Ok(())
    }

    pub fn message(&mut self, text: &str, by: AccountId, at: u64) -> Result<(), ContextError> {
        self.require_member(by)?;
        self.events.push(ContextEvent::Message {
            text: text.to_string(),
            by,
            at,
        });
        Ok(())
    }

    pub fn to_blob(&self) -> Result<Blob, serde_json::Error> {
        serde_json::to_vec(self).map(|bytes| Blob::new(bytes, CONTEXT_MIME))
    }

    pub fn from_blob(blob: &Blob) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(&blob.data)
    }

    fn require_member(&self, account: AccountId) -> Result<(), ContextError> {
        if self.is_member(&account) {
            Ok(())
        } else {
            Err(ContextError::NotMember(account))
        }
    }
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextError::NotMember(account) => {
                write!(f, "account {} is not a member", hex_cid(account))
            }
            ContextError::MissingNode(cid) => write!(f, "node {} is not in context", hex_cid(cid)),
        }
    }
}

impl std::error::Error for ContextError {}

fn context_id(name: &str, owner: &AccountId, at: u64) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(name.as_bytes());
    hasher.update(owner);
    hasher.update(&at.to_le_bytes());
    let hash = *hasher.finalize().as_bytes();
    format!("ctx-{}", &hex_cid(&hash)[..12])
}

fn current_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
