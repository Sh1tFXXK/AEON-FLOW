use crate::{Blob, CID};
use serde::{Deserialize, Serialize};

pub const MESSAGE_MIME: &str = "application/x-aeon-message";
pub const THREAD_MIME: &str = "application/x-aeon-thread";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Message {
    pub cid: CID,
    pub thread_id: String,
    pub author: String,
    pub content_cid: CID,
    pub reply_to: Option<CID>,
    pub at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Thread {
    pub id: String,
    pub messages: Vec<CID>,
    pub participants: Vec<String>,
    pub context_id: Option<String>,
}

impl Message {
    pub fn new(
        thread_id: &str,
        author: &str,
        content_cid: CID,
        reply_to: Option<CID>,
        at: u64,
    ) -> Self {
        let mut message = Self {
            cid: [0u8; 32],
            thread_id: thread_id.to_string(),
            author: author.to_string(),
            content_cid,
            reply_to,
            at,
        };
        message.recompute_cid();
        message
    }

    pub fn to_blob(&self) -> Result<Blob, serde_json::Error> {
        serde_json::to_vec(self).map(|bytes| Blob::new(bytes, MESSAGE_MIME))
    }

    pub fn from_blob(blob: &Blob) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(&blob.data)
    }

    fn recompute_cid(&mut self) {
        let mut hasher = blake3::Hasher::new();
        update_str(&mut hasher, &self.thread_id);
        update_str(&mut hasher, &self.author);
        hasher.update(&self.content_cid);
        match self.reply_to {
            Some(cid) => {
                hasher.update(&[1]);
                hasher.update(&cid);
            }
            None => {
                hasher.update(&[0]);
            }
        }
        hasher.update(&self.at.to_le_bytes());
        self.cid = *hasher.finalize().as_bytes();
    }
}

impl Thread {
    pub fn new(id: &str, participants: Vec<String>, context_id: Option<String>) -> Self {
        Self {
            id: id.to_string(),
            messages: Vec::new(),
            participants,
            context_id,
        }
    }

    pub fn add_message(&mut self, message: &Message) {
        if !self.messages.contains(&message.cid) {
            self.messages.push(message.cid);
        }
    }

    pub fn to_blob(&self) -> Result<Blob, serde_json::Error> {
        serde_json::to_vec(self).map(|bytes| Blob::new(bytes, THREAD_MIME))
    }

    pub fn from_blob(blob: &Blob) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(&blob.data)
    }
}

fn update_str(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
