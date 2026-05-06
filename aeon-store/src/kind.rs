use crate::{Blob, CID};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DataKind {
    PlainText,
    Markdown,
    Code {
        language: String,
    },
    Json,
    Image {
        width: u32,
        height: u32,
        format: String,
    },
    Audio {
        duration_secs: f32,
        format: String,
    },
    Video {
        duration_secs: f32,
        format: String,
    },
    VMSnapshot,
    VMProgram,
    ForthScript,
    PythonScript,
    ConversationMessage {
        thread_id: String,
    },
    ConversationThread,
    Blob,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DataDescriptor {
    pub blob_cid: CID,
    pub kind: DataKind,
    pub mime: String,
    pub size_bytes: usize,
}

impl DataDescriptor {
    pub fn from_blob(path: Option<&Path>, blob: &Blob) -> Self {
        Self {
            blob_cid: blob.cid,
            kind: DataKind::from_path_and_mime(path, &blob.mime),
            mime: blob.mime.clone(),
            size_bytes: blob.data.len(),
        }
    }
}

impl DataKind {
    pub fn from_path_and_mime(path: Option<&Path>, mime: &str) -> Self {
        let ext = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        match ext.as_deref() {
            Some("md") => Self::Markdown,
            Some("json") => Self::Json,
            Some("rs") => Self::Code {
                language: "rust".to_string(),
            },
            Some("py") => Self::PythonScript,
            Some("fs") | Some("forth") => Self::ForthScript,
            Some("aeon") => Self::VMProgram,
            Some("snap") => Self::VMSnapshot,
            Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") => Self::Image {
                width: 0,
                height: 0,
                format: ext.clone().unwrap_or_else(|| "image".to_string()),
            },
            Some("mp3") | Some("wav") | Some("flac") | Some("ogg") => Self::Audio {
                duration_secs: 0.0,
                format: ext.clone().unwrap_or_else(|| "audio".to_string()),
            },
            Some("mp4") | Some("mov") | Some("mkv") | Some("webm") => Self::Video {
                duration_secs: 0.0,
                format: ext.clone().unwrap_or_else(|| "video".to_string()),
            },
            _ if mime == "text/plain" => Self::PlainText,
            _ if mime == "application/json" => Self::Json,
            _ if mime == "application/x-aeon-context" => Self::ConversationThread,
            _ => Self::Blob,
        }
    }
}
