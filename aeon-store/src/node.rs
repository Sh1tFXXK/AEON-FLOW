use crate::{Link, CID};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Node {
    pub cid: CID,
    pub blob_cid: CID,
    pub kind: String,
    pub name: Option<String>,
    pub created_at: u64,
    pub created_by: String,
    pub tags: Vec<String>,
    pub links: Vec<Link>,
}

impl Node {
    pub fn new(blob_cid: CID, kind: &str, created_by: &str) -> Self {
        let created_at = current_millis();
        let mut node = Self {
            cid: [0u8; 32],
            blob_cid,
            kind: kind.to_string(),
            name: None,
            created_at,
            created_by: created_by.to_string(),
            tags: Vec::new(),
            links: Vec::new(),
        };
        node.recompute_cid();
        node
    }

    pub fn with_name(mut self, name: &str) -> Self {
        self.name = Some(name.to_string());
        self.recompute_cid();
        self
    }

    pub fn with_tag(mut self, tag: &str) -> Self {
        self.tags.push(tag.to_string());
        self.recompute_cid();
        self
    }

    pub fn link_to(mut self, target: CID, label: &str) -> Self {
        self.links.push(Link::new(target, label));
        self.recompute_cid();
        self
    }

    fn recompute_cid(&mut self) {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.blob_cid);
        update_str(&mut hasher, &self.kind);
        update_opt_str(&mut hasher, self.name.as_deref());
        hasher.update(&self.created_at.to_le_bytes());
        update_str(&mut hasher, &self.created_by);

        hasher.update(&(self.tags.len() as u64).to_le_bytes());
        for tag in &self.tags {
            update_str(&mut hasher, tag);
        }

        hasher.update(&(self.links.len() as u64).to_le_bytes());
        for link in &self.links {
            hasher.update(&link.target_cid);
            update_str(&mut hasher, &link.label);
        }

        self.cid = *hasher.finalize().as_bytes();
    }
}

fn current_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn update_opt_str(hasher: &mut blake3::Hasher, value: Option<&str>) {
    match value {
        Some(value) => {
            hasher.update(&[1]);
            update_str(hasher, value);
        }
        None => {
            hasher.update(&[0]);
        }
    };
}

fn update_str(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(&(value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
