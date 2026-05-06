use crate::CID;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Blob {
    pub cid: CID,
    pub data: Vec<u8>,
    pub mime: String,
}

impl Blob {
    pub fn new(data: Vec<u8>, mime: &str) -> Self {
        let cid = *blake3::hash(&data).as_bytes();
        Self {
            cid,
            data,
            mime: mime.to_string(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        Self::new(text.as_bytes().to_vec(), "text/plain")
    }

    pub fn from_json(value: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::to_vec(value).map(|bytes| Self::new(bytes, "application/json"))
    }

    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let data = std::fs::read(path)?;
        let mime = mime_from_path(path);
        Ok(Self::new(data, &mime))
    }

    pub fn as_text(&self) -> Option<&str> {
        std::str::from_utf8(&self.data).ok()
    }

    pub fn as_json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.data).ok()
    }
}

pub fn mime_from_path(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        Some("txt") | Some("md") => "text/plain",
        Some("json") => "application/json",
        Some("rs") => "text/x-rust",
        Some("py") => "text/x-python",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("aeon") => "application/x-aeon-program",
        Some("snap") => "application/x-aeon-snapshot",
        _ => "application/octet-stream",
    }
    .to_string()
}
