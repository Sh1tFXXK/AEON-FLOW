use crate::capture::{CaptureEntry, CaptureKind, CaptureSource, CID};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EventId(pub [u8; 32]);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AeonEvent {
    pub id: EventId,
    pub ts: u64,
    pub kind: EventKind,
    pub source: EventSource,
    pub device: [u8; 16],
    pub identity: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventKind {
    CaptureAdded(CaptureEvent),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureEvent {
    pub cid: CID,
    pub capture_kind: CaptureKind,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub app_name: Option<String>,
    pub file_path: Option<String>,
    pub url: Option<String>,
    pub size: usize,
    pub mime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EventSource {
    LocalCapture(CaptureSource),
    RelayImport { device_name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseEventIdError;

impl EventId {
    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    pub fn from_hex(value: &str) -> Result<Self, ParseEventIdError> {
        if value.len() != 64 {
            return Err(ParseEventIdError);
        }

        let mut bytes = [0u8; 32];
        for index in 0..32 {
            let start = index * 2;
            let end = start + 2;
            bytes[index] =
                u8::from_str_radix(&value[start..end], 16).map_err(|_| ParseEventIdError)?;
        }
        Ok(Self(bytes))
    }
}

impl AeonEvent {
    pub fn from_capture(entry: &CaptureEntry) -> Self {
        let source = match &entry.source {
            CaptureSource::PeerSync { device_name } => EventSource::RelayImport {
                device_name: device_name.clone(),
            },
            source => EventSource::LocalCapture(source.clone()),
        };
        let kind = EventKind::CaptureAdded(CaptureEvent {
            cid: entry.cid,
            capture_kind: entry.kind.clone(),
            title: entry.meta.title.clone(),
            summary: entry.meta.summary.clone(),
            app_name: entry.meta.app_name.clone(),
            file_path: entry.meta.file_path.clone(),
            url: entry.meta.url.clone(),
            size: entry.data.len(),
            mime: entry.mime(),
        });

        let id = build_event_id(entry.captured_at, &kind, &source, &entry.device, &entry.by);
        Self {
            id,
            ts: entry.captured_at,
            kind,
            source,
            device: entry.device,
            identity: entry.by,
        }
    }
}

fn build_event_id(
    ts: u64,
    kind: &EventKind,
    source: &EventSource,
    device: &[u8; 16],
    identity: &[u8; 32],
) -> EventId {
    let mut hasher = blake3::Hasher::new();
    hasher.update(&ts.to_be_bytes());
    hasher.update(device);
    hasher.update(identity);
    if let Ok(bytes) = serde_json::to_vec(kind) {
        hasher.update(&bytes);
    }
    if let Ok(bytes) = serde_json::to_vec(source) {
        hasher.update(&bytes);
    }
    EventId(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture_entry() -> CaptureEntry {
        let mut entry = CaptureEntry::new(
            b"hello event".to_vec(),
            CaptureKind::Text,
            CaptureSource::Manual,
        )
        .with_title("Greeting")
        .with_summary("hello event");
        entry.by = [7u8; 32];
        entry.device = [9u8; 16];
        entry.meta.app_name = Some("Test App".to_string());
        entry.meta.file_path = Some("E:/tmp/example.txt".to_string());
        entry.meta.url = Some("https://example.test".to_string());
        entry
    }

    #[test]
    fn capture_projection_copies_metadata_without_raw_bytes() {
        let entry = capture_entry();
        let event = AeonEvent::from_capture(&entry);

        assert_eq!(event.ts, entry.captured_at);
        assert_eq!(event.device, [9u8; 16]);
        assert_eq!(event.identity, [7u8; 32]);
        assert_eq!(
            event.source,
            EventSource::LocalCapture(CaptureSource::Manual)
        );

        let EventKind::CaptureAdded(capture) = event.kind;
        assert_eq!(capture.cid, entry.cid);
        assert_eq!(capture.capture_kind, CaptureKind::Text);
        assert_eq!(capture.title.as_deref(), Some("Greeting"));
        assert_eq!(capture.summary.as_deref(), Some("hello event"));
        assert_eq!(capture.app_name.as_deref(), Some("Test App"));
        assert_eq!(capture.file_path.as_deref(), Some("E:/tmp/example.txt"));
        assert_eq!(capture.url.as_deref(), Some("https://example.test"));
        assert_eq!(capture.size, b"hello event".len());
        assert_eq!(capture.mime, "text/plain");
    }

    #[test]
    fn peer_sync_capture_projects_to_relay_import_source() {
        let mut entry = CaptureEntry::new(
            b"remote".to_vec(),
            CaptureKind::Text,
            CaptureSource::PeerSync {
                device_name: "Phone".to_string(),
            },
        );
        entry.by = [1u8; 32];
        entry.device = [2u8; 16];

        let event = AeonEvent::from_capture(&entry);

        assert_eq!(
            event.source,
            EventSource::RelayImport {
                device_name: "Phone".to_string()
            }
        );
    }

    #[test]
    fn event_id_is_deterministic_and_hex_round_trips() {
        let entry = capture_entry();
        let first = AeonEvent::from_capture(&entry);
        let second = AeonEvent::from_capture(&entry);

        assert_eq!(first.id, second.id);

        let hex = first.id.to_hex();
        assert_eq!(hex.len(), 64);
        assert_eq!(EventId::from_hex(&hex).unwrap(), first.id);
        assert!(EventId::from_hex("abc").is_err());
        assert!(EventId::from_hex(
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"
        )
        .is_err());
    }
}
