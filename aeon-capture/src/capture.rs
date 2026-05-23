use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type CID = aeon_store::CID;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureEntry {
    pub cid: CID,
    pub kind: CaptureKind,
    pub data: Vec<u8>,
    pub meta: CaptureMetadata,
    pub source: CaptureSource,
    pub captured_at: u64,
    pub by: [u8; 32],
    pub device: [u8; 16],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CaptureKind {
    Conversation,
    Code {
        language: String,
    },
    Text,
    Image {
        width: u32,
        height: u32,
        format: String,
    },
    Webpage,
    Document {
        format: String,
    },
    ProcessState,
    OsActivity,
    VmSnapshot,
    Clipboard,
    Blob {
        mime: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CaptureMetadata {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub app_name: Option<String>,
    pub file_path: Option<String>,
    pub url: Option<String>,
    pub message_count: Option<usize>,
    pub previous_version: Option<CID>,
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OsCaptureProvider {
    WinEventHook,
    WindowsUiAutomation,
    ShellNotification,
    FilesystemWatcher,
    ClipboardApi,
    BrowserBridge,
    AndroidSystemBridge,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CaptureSource {
    DragDrop,
    Clipboard,
    Screenshot,
    FileWatch { path: String },
    AppApi { app: String },
    OperatingSystem { provider: OsCaptureProvider },
    ShareMenu,
    Manual,
    PeerSync { device_name: String },
}

impl CaptureEntry {
    pub fn new(data: Vec<u8>, kind: CaptureKind, source: CaptureSource) -> Self {
        let cid = *blake3::hash(&data).as_bytes();
        let captured_at = now_ms();

        CaptureEntry {
            cid,
            kind,
            data,
            meta: CaptureMetadata::default(),
            source,
            captured_at,
            by: [0u8; 32],
            device: [0u8; 16],
        }
    }

    pub fn with_title(mut self, title: &str) -> Self {
        self.meta.title = Some(title.to_string());
        self
    }

    pub fn with_summary(mut self, summary: &str) -> Self {
        self.meta.summary = Some(summary.chars().take(200).collect());
        self
    }

    pub fn with_app(mut self, app: &str) -> Self {
        self.meta.app_name = Some(app.to_string());
        self
    }

    pub fn with_prev(mut self, prev: CID) -> Self {
        self.meta.previous_version = Some(prev);
        self
    }

    pub fn mime(&self) -> String {
        self.kind.mime()
    }

    pub fn description(&self) -> String {
        match &self.kind {
            CaptureKind::Conversation => format!(
                "对话 ({} 条消息) - {}",
                self.meta.message_count.unwrap_or(0),
                self.meta.app_name.as_deref().unwrap_or("未知应用")
            ),
            CaptureKind::Code { language } => format!(
                "{} 代码 - {}",
                language,
                self.meta.title.as_deref().unwrap_or("未命名")
            ),
            CaptureKind::Image { width, height, .. } => format!(
                "图片 {}x{} - {}",
                width,
                height,
                self.meta.title.as_deref().unwrap_or("截图")
            ),
            CaptureKind::Clipboard => format!(
                "剪贴板 - {}",
                self.meta.summary.as_deref().unwrap_or("内容")
            ),
            CaptureKind::VmSnapshot => format!(
                "AEON VM 快照 - {}",
                self.meta.title.as_deref().unwrap_or("未命名")
            ),
            _ => self
                .meta
                .title
                .clone()
                .unwrap_or_else(|| format!("{:?}", self.kind)),
        }
    }
}

impl CaptureKind {
    pub fn key(&self) -> &'static str {
        match self {
            CaptureKind::Conversation => "Conversation",
            CaptureKind::Code { .. } => "Code",
            CaptureKind::Text => "Text",
            CaptureKind::Image { .. } => "Image",
            CaptureKind::Webpage => "Webpage",
            CaptureKind::Document { .. } => "Document",
            CaptureKind::ProcessState => "ProcessState",
            CaptureKind::OsActivity => "OsActivity",
            CaptureKind::VmSnapshot => "VmSnapshot",
            CaptureKind::Clipboard => "Clipboard",
            CaptureKind::Blob { .. } => "Blob",
        }
    }

    pub fn mime(&self) -> String {
        match self {
            CaptureKind::Conversation => "application/vnd.aeon.conversation+json".to_string(),
            CaptureKind::Code { language } => format!("text/x-{}", language.to_lowercase()),
            CaptureKind::Text | CaptureKind::Clipboard => "text/plain".to_string(),
            CaptureKind::Image { format, .. } => format!("image/{format}"),
            CaptureKind::Webpage => "application/vnd.aeon.webpage+json".to_string(),
            CaptureKind::Document { format } => format!("application/{format}"),
            CaptureKind::ProcessState => "application/vnd.aeon.process-state".to_string(),
            CaptureKind::OsActivity => "application/vnd.aeon.os-activity+json".to_string(),
            CaptureKind::VmSnapshot => "application/x-aeon-snapshot".to_string(),
            CaptureKind::Blob { mime } => mime.clone(),
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_truncates_on_char_boundary() {
        let entry = CaptureEntry::new(Vec::new(), CaptureKind::Text, CaptureSource::Manual)
            .with_summary(&"你好".repeat(150));

        assert_eq!(entry.meta.summary.unwrap().chars().count(), 200);
    }

    #[test]
    fn os_activity_has_typed_kind_and_source_provider() {
        assert_eq!(CaptureKind::OsActivity.key(), "OsActivity");
        assert_eq!(
            CaptureKind::OsActivity.mime(),
            "application/vnd.aeon.os-activity+json"
        );

        let source = CaptureSource::OperatingSystem {
            provider: OsCaptureProvider::WinEventHook,
        };

        assert_eq!(
            source,
            CaptureSource::OperatingSystem {
                provider: OsCaptureProvider::WinEventHook
            }
        );
    }

    #[test]
    fn description_uses_conversation_metadata() {
        let mut entry =
            CaptureEntry::new(Vec::new(), CaptureKind::Conversation, CaptureSource::Manual);
        entry.meta.message_count = Some(3);
        entry.meta.app_name = Some("Claude Desktop".to_string());

        assert_eq!(entry.description(), "对话 (3 条消息) - Claude Desktop");
    }
}
