# AEON Event Timeline Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an append-only typed event timeline that records every successful capture without replacing the existing capture store.

**Architecture:** `CaptureEntry` remains the content object and CIDStore remains the content layer. `AeonEvent` is a separate temporal projection stored in a JSONL `EventLog`, and `CaptureEngine` appends one event only after a capture is stored successfully. `aeon-sync` exposes read-only event APIs through the existing Axum server state.

**Tech Stack:** Rust 2021, serde, serde_json, blake3, tokio, axum, existing `aeon-capture` and `aeon-sync` crates.

---

## File Structure

- Create `aeon-capture/src/event.rs`
  - Owns `EventId`, `AeonEvent`, `EventKind`, `CaptureEvent`, `EventSource`, event ID hex parsing, and `CaptureEntry` projection.
- Create `aeon-capture/src/event_log.rs`
  - Owns append-only JSONL persistence, newest-first listing, range filtering, event lookup, corrupt-line tolerance, and default query limits.
- Modify `aeon-capture/src/lib.rs`
  - Exports `event` and `event_log` modules plus public event types.
- Modify `aeon-capture/src/engine.rs`
  - Stores an optional event log owner and appends `AeonEvent` after successful `CaptureStore::put`.
- Modify `aeon-sync/src/main.rs`
  - Opens `~/.aeon/events.jsonl`, passes it to `CaptureEngine`, and stores it in `AppState`.
- Modify `aeon-sync/src/server.rs`
  - Adds event log state, `GET /api/events`, `GET /api/events/:id`, API payload conversion, and query validation tests.

No root `Cargo.toml` exists, so verification commands run inside each crate directory.

---

### Task 1: Event Model And Capture Projection

**Files:**
- Create: `aeon-capture/src/event.rs`
- Modify: `aeon-capture/src/lib.rs`
- Test: `aeon-capture/src/event.rs`

- [ ] **Step 1: Write failing event model tests**

Create `aeon-capture/src/event.rs` with the tests first:

```rust
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
        assert_eq!(event.source, EventSource::LocalCapture(CaptureSource::Manual));

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
        assert!(EventId::from_hex("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz").is_err());
    }
}
```

- [ ] **Step 2: Run the failing tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test event::tests -- --nocapture
```

Expected: compile fails because `AeonEvent::from_capture`, `EventId::to_hex`, and `EventId::from_hex` are not defined.

- [ ] **Step 3: Implement the event model**

Replace the non-test portion of `aeon-capture/src/event.rs` with this implementation while keeping the tests from Step 1:

```rust
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
            bytes[index] = u8::from_str_radix(&value[start..end], 16)
                .map_err(|_| ParseEventIdError)?;
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
```

- [ ] **Step 4: Export the event types**

Modify `aeon-capture/src/lib.rs`:

```rust
pub mod apps;
pub mod capture;
pub mod clipboard;
pub mod engine;
pub mod event;
pub mod file;
pub mod screenshot;
pub mod store;

pub use aeon_store::{hex_cid, parse_cid_hex};
pub use capture::{CaptureEntry, CaptureKind, CaptureMetadata, CaptureSource, CID};
pub use engine::CaptureEngine;
pub use event::{AeonEvent, CaptureEvent, EventId, EventKind, EventSource};
pub use store::{CaptureRecord, CaptureStore};
```

- [ ] **Step 5: Run the event model tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test event::tests -- --nocapture
```

Expected: all `event::tests` pass.

- [ ] **Step 6: Commit Task 1**

```powershell
cd E:\project\AEON-FLOW
git add aeon-capture/src/event.rs aeon-capture/src/lib.rs
git commit -m "feat: add capture event model"
```

---

### Task 2: Append-Only Event Log

**Files:**
- Create: `aeon-capture/src/event_log.rs`
- Modify: `aeon-capture/src/lib.rs`
- Test: `aeon-capture/src/event_log.rs`

- [ ] **Step 1: Write failing event log tests**

Create `aeon-capture/src/event_log.rs`:

```rust
use crate::event::{AeonEvent, EventId};
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventQuery {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: usize,
}

pub struct EventLog {
    path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
    use crate::event::{EventKind, EventSource};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-event-log-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn event_with_ts(ts: u64, text: &str) -> AeonEvent {
        let mut entry =
            CaptureEntry::new(text.as_bytes().to_vec(), CaptureKind::Text, CaptureSource::Manual);
        entry.captured_at = ts;
        entry.by = [3u8; 32];
        entry.device = [4u8; 16];
        AeonEvent::from_capture(&entry)
    }

    #[test]
    fn append_writes_one_json_line_per_event_and_lists_newest_first() {
        let dir = temp_dir();
        let log = EventLog::new(dir.join("events.jsonl"));
        let older = event_with_ts(100, "older");
        let newer = event_with_ts(200, "newer");

        log.append(&older).unwrap();
        log.append(&newer).unwrap();

        let raw = std::fs::read_to_string(dir.join("events.jsonl")).unwrap();
        assert_eq!(raw.lines().count(), 2);

        let events = log.list(EventQuery {
            from: None,
            to: None,
            limit: 10,
        }).unwrap();
        assert_eq!(events.iter().map(|event| event.ts).collect::<Vec<_>>(), vec![200, 100]);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn range_query_filters_by_timestamp_and_limit() {
        let dir = temp_dir();
        let log = EventLog::new(dir.join("events.jsonl"));
        log.append(&event_with_ts(100, "a")).unwrap();
        log.append(&event_with_ts(200, "b")).unwrap();
        log.append(&event_with_ts(300, "c")).unwrap();

        let events = log.list(EventQuery {
            from: Some(150),
            to: Some(350),
            limit: 1,
        }).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].ts, 300);
        let EventKind::CaptureAdded(capture) = &events[0].kind;
        assert_eq!(capture.mime, "text/plain");
        assert_eq!(events[0].source, EventSource::LocalCapture(CaptureSource::Manual));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_lines_are_skipped_during_reads() {
        let dir = temp_dir();
        let path = dir.join("events.jsonl");
        let valid = event_with_ts(100, "valid");
        std::fs::write(
            &path,
            format!("not-json\n{}\n", serde_json::to_string(&valid).unwrap()),
        )
        .unwrap();

        let log = EventLog::new(&path);
        let events = log.list(EventQuery::default()).unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, valid.id);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn get_finds_event_by_id() {
        let dir = temp_dir();
        let log = EventLog::new(dir.join("events.jsonl"));
        let event = event_with_ts(100, "find me");
        let id = event.id;
        log.append(&event).unwrap();

        assert_eq!(log.get(&id).unwrap().unwrap().id, id);
        assert!(log.get(&EventId([0u8; 32])).unwrap().is_none());

        let _ = std::fs::remove_dir_all(dir);
    }
}
```

- [ ] **Step 2: Run the failing event log tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test event_log::tests -- --nocapture
```

Expected: compile fails because `EventLog::new`, `append`, `list`, `get`, and `EventQuery::default` are not defined, and the module is not exported yet.

- [ ] **Step 3: Implement EventLog**

Replace the non-test portion of `aeon-capture/src/event_log.rs` with:

```rust
use crate::event::{AeonEvent, EventId};
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

const DEFAULT_EVENT_LIMIT: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventQuery {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: usize,
}

impl Default for EventQuery {
    fn default() -> Self {
        Self {
            from: None,
            to: None,
            limit: DEFAULT_EVENT_LIMIT,
        }
    }
}

pub struct EventLog {
    path: PathBuf,
}

impl EventLog {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn append(&self, event: &AeonEvent) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(event)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        file.write_all(line.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()
    }

    pub fn list(&self, query: EventQuery) -> io::Result<Vec<AeonEvent>> {
        let mut events = self.read_all()?;
        events.retain(|event| {
            query.from.map_or(true, |from| event.ts >= from)
                && query.to.map_or(true, |to| event.ts <= to)
        });
        events.sort_by(|a, b| b.ts.cmp(&a.ts).then(a.id.0.cmp(&b.id.0)));
        events.truncate(query.limit);
        Ok(events)
    }

    pub fn get(&self, id: &EventId) -> io::Result<Option<AeonEvent>> {
        Ok(self.read_all()?.into_iter().find(|event| &event.id == id))
    }

    fn read_all(&self) -> io::Result<Vec<AeonEvent>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = std::fs::File::open(&self.path)?;
        let reader = std::io::BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let Ok(line) = line else {
                continue;
            };
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<AeonEvent>(&line) {
                events.push(event);
            }
        }

        Ok(events)
    }
}
```

- [ ] **Step 4: Export the event log types**

Modify `aeon-capture/src/lib.rs`:

```rust
pub mod apps;
pub mod capture;
pub mod clipboard;
pub mod engine;
pub mod event;
pub mod event_log;
pub mod file;
pub mod screenshot;
pub mod store;

pub use aeon_store::{hex_cid, parse_cid_hex};
pub use capture::{CaptureEntry, CaptureKind, CaptureMetadata, CaptureSource, CID};
pub use engine::CaptureEngine;
pub use event::{AeonEvent, CaptureEvent, EventId, EventKind, EventSource};
pub use event_log::{EventLog, EventQuery};
pub use store::{CaptureRecord, CaptureStore};
```

- [ ] **Step 5: Run EventLog tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test event_log::tests -- --nocapture
```

Expected: all `event_log::tests` pass.

- [ ] **Step 6: Commit Task 2**

```powershell
cd E:\project\AEON-FLOW
git add aeon-capture/src/event_log.rs aeon-capture/src/lib.rs
git commit -m "feat: add append-only event log"
```

---

### Task 3: CaptureEngine Event Append Pipeline

**Files:**
- Modify: `aeon-capture/src/engine.rs`
- Test: `aeon-capture/src/engine.rs`

- [ ] **Step 1: Write failing engine tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `aeon-capture/src/engine.rs`:

```rust
    #[tokio::test]
    async fn capture_appends_event_after_store_success() {
        let dir = temp_dir();
        let store = CaptureStore::new(
            CIDStore::new(dir.join("store")).unwrap(),
            dir.join("index.json"),
        )
        .unwrap();
        let event_log = Arc::new(Mutex::new(crate::event_log::EventLog::new(
            dir.join("events.jsonl"),
        )));
        let engine = CaptureEngine::new_with_identity_and_events(
            Arc::new(Mutex::new(store)),
            [11u8; 32],
            [12u8; 16],
            Some(event_log.clone()),
        );

        let cid = engine
            .capture(CaptureEntry::new(
                b"eventful".to_vec(),
                CaptureKind::Text,
                CaptureSource::Manual,
            ))
            .await
            .unwrap();

        let events = event_log
            .lock()
            .await
            .list(crate::event_log::EventQuery::default())
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].identity, [11u8; 32]);
        assert_eq!(events[0].device, [12u8; 16]);
        let crate::event::EventKind::CaptureAdded(capture) = &events[0].kind;
        assert_eq!(capture.cid, cid);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn failed_capture_store_write_does_not_append_event() {
        let dir = temp_dir();
        let blocked_parent = dir.join("blocked-parent");
        std::fs::write(&blocked_parent, b"not a directory").unwrap();
        let store = CaptureStore::new(
            CIDStore::new(dir.join("store")).unwrap(),
            blocked_parent.join("index.json"),
        )
        .unwrap();
        let event_log = Arc::new(Mutex::new(crate::event_log::EventLog::new(
            dir.join("events.jsonl"),
        )));
        let engine = CaptureEngine::new_with_identity_and_events(
            Arc::new(Mutex::new(store)),
            [11u8; 32],
            [12u8; 16],
            Some(event_log.clone()),
        );

        let result = engine
            .capture(CaptureEntry::new(
                b"will not index".to_vec(),
                CaptureKind::Text,
                CaptureSource::Manual,
            ))
            .await;

        assert!(result.is_err());
        let events = event_log
            .lock()
            .await
            .list(crate::event_log::EventQuery::default())
            .unwrap();
        assert!(events.is_empty());

        let _ = std::fs::remove_dir_all(dir);
    }
```

- [ ] **Step 2: Run the failing engine tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test engine::tests -- --nocapture
```

Expected: compile fails because `CaptureEngine::new_with_identity_and_events` is not defined.

- [ ] **Step 3: Add event log ownership to CaptureEngine**

Modify the imports and struct in `aeon-capture/src/engine.rs`:

```rust
use crate::capture::{CaptureEntry, CaptureKind, CID};
use crate::event::AeonEvent;
use crate::event_log::EventLog;
use crate::store::{CaptureRecord, CaptureStore};
use aeon_store::Blob;
use std::io;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

pub struct CaptureEngine {
    store: Arc<Mutex<CaptureStore>>,
    event_log: Option<Arc<Mutex<EventLog>>>,
    tx: broadcast::Sender<CaptureEntry>,
    by: [u8; 32],
    device: [u8; 16],
}
```

Update the constructors:

```rust
impl CaptureEngine {
    pub fn new(store: Arc<Mutex<CaptureStore>>) -> Self {
        Self::new_with_identity_and_events(store, [0u8; 32], [0u8; 16], None)
    }

    pub fn new_with_identity(
        store: Arc<Mutex<CaptureStore>>,
        by: [u8; 32],
        device: [u8; 16],
    ) -> Self {
        Self::new_with_identity_and_events(store, by, device, None)
    }

    pub fn new_with_identity_and_events(
        store: Arc<Mutex<CaptureStore>>,
        by: [u8; 32],
        device: [u8; 16],
        event_log: Option<Arc<Mutex<EventLog>>>,
    ) -> Self {
        let (tx, _) = broadcast::channel(256);
        CaptureEngine {
            store,
            event_log,
            tx,
            by,
            device,
        }
    }
```

- [ ] **Step 4: Append events after successful capture storage**

Modify `CaptureEngine::capture`:

```rust
    pub async fn capture(&self, mut entry: CaptureEntry) -> io::Result<CID> {
        let cid = entry.cid;
        self.stamp_identity(&mut entry);
        self.enrich(&mut entry);
        crate::apps::auto_wrap_capture_entry(&mut entry)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        {
            let mut store = self.store.lock().await;
            store.put(entry.clone())?;
        }

        if let Some(event_log) = &self.event_log {
            let event = AeonEvent::from_capture(&entry);
            event_log.lock().await.append(&event)?;
        }

        let _ = self.tx.send(entry);
        Ok(cid)
    }
```

- [ ] **Step 5: Run engine tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test engine::tests -- --nocapture
```

Expected: all engine tests pass.

- [ ] **Step 6: Run all aeon-capture tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test
```

Expected: all `aeon-capture` tests pass.

- [ ] **Step 7: Commit Task 3**

```powershell
cd E:\project\AEON-FLOW
git add aeon-capture/src/engine.rs
git commit -m "feat: append capture events from engine"
```

---

### Task 4: aeon-sync Event API Wiring

**Files:**
- Modify: `aeon-sync/src/main.rs`
- Modify: `aeon-sync/src/server.rs`
- Test: `aeon-sync/src/server.rs`

- [ ] **Step 1: Write failing server query tests**

In `aeon-sync/src/server.rs`, add `Query` to the Axum extract imports:

```rust
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Multipart, Path, Query, State,
    },
```

Add these structs near the existing payload structs:

```rust
#[derive(Deserialize, Default)]
pub struct EventListParams {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: Option<usize>,
}
```

Add this test inside the existing `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn event_list_params_enforce_bounded_nonzero_limit() {
        let default_query = EventListParams::default().try_into_query().unwrap();
        assert_eq!(default_query.limit, 100);

        let capped_query = EventListParams {
            from: Some(10),
            to: Some(20),
            limit: Some(5000),
        }
        .try_into_query()
        .unwrap();
        assert_eq!(capped_query.from, Some(10));
        assert_eq!(capped_query.to, Some(20));
        assert_eq!(capped_query.limit, 500);

        let zero_limit = EventListParams {
            from: None,
            to: None,
            limit: Some(0),
        };
        assert!(zero_limit.try_into_query().is_err());
    }
```

- [ ] **Step 2: Run the failing aeon-sync tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-sync
cargo test server::tests::event_list_params_enforce_bounded_nonzero_limit -- --nocapture
```

Expected: compile fails because `try_into_query` is not defined and event types are not imported.

- [ ] **Step 3: Import EventLog and EventQuery**

Update the `aeon_capture` import in `aeon-sync/src/server.rs`:

```rust
    hex_cid, parse_cid_hex, AeonEvent, CaptureEngine, CaptureEntry, CaptureKind, CaptureRecord,
    CaptureSource, EventId, EventLog, EventQuery, CID,
```

Add the event log to `AppState`:

```rust
    pub event_log: Arc<Mutex<EventLog>>,
```

Add event API payload structs near `CaptureDetailPayload`:

```rust
#[derive(Serialize)]
pub struct EventPayload {
    pub id: String,
    pub ts: u64,
    pub kind: aeon_capture::EventKind,
    pub source: aeon_capture::EventSource,
    pub device: String,
    pub identity: String,
}
```

- [ ] **Step 4: Implement query conversion and handlers**

Add these functions near `list_entries`:

```rust
impl EventListParams {
    fn try_into_query(self) -> Result<EventQuery, StatusCode> {
        let limit = self.limit.unwrap_or(100);
        if limit == 0 {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(EventQuery {
            from: self.from,
            to: self.to,
            limit: limit.min(500),
        })
    }
}

pub async fn list_events(
    Query(params): Query<EventListParams>,
    State(state): State<AppState>,
) -> Result<Json<Vec<EventPayload>>, StatusCode> {
    let query = params.try_into_query()?;
    let events = state
        .event_log
        .lock()
        .await
        .list(query)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(events.into_iter().map(event_payload).collect()))
}

pub async fn get_event(
    Path(id_hex): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<EventPayload>, StatusCode> {
    let id = EventId::from_hex(&id_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
    let event = state
        .event_log
        .lock()
        .await
        .get(&id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(event_payload(event)))
}

fn event_payload(event: AeonEvent) -> EventPayload {
    EventPayload {
        id: event.id.to_hex(),
        ts: event.ts,
        kind: event.kind,
        source: event.source,
        device: hex_bytes(&event.device),
        identity: hex_bytes(&event.identity),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
```

If `hex_bytes` conflicts with an existing function name in `server.rs`, keep this helper private in `server.rs` and rename it to `hex_bytes_local`.

- [ ] **Step 5: Register event routes**

Modify `create_router` in `aeon-sync/src/server.rs`:

```rust
        .route("/api/events", get(list_events))
        .route("/api/events/:id", get(get_event))
```

Place both routes after `/api/devices/hello` and before `/api/entries`.

- [ ] **Step 6: Wire EventLog in main**

Update imports in `aeon-sync/src/main.rs`:

```rust
use aeon_capture::{apps, CaptureEngine, CaptureStore, EventLog};
```

After `let aeon_dir = home.join(".aeon");`, add:

```rust
    let event_log = Arc::new(Mutex::new(EventLog::new(aeon_dir.join("events.jsonl"))));
```

Change the capture engine construction:

```rust
    let capture_engine = Arc::new(CaptureEngine::new_with_identity_and_events(
        Arc::new(Mutex::new(capture_store)),
        identity.id,
        device_id,
        Some(event_log.clone()),
    ));
```

Add `event_log` to `server::AppState` construction:

```rust
        event_log,
```

- [ ] **Step 7: Run aeon-sync tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-sync
cargo test
```

Expected: all `aeon-sync` tests pass.

- [ ] **Step 8: Commit Task 4**

```powershell
cd E:\project\AEON-FLOW
git add aeon-sync/src/main.rs aeon-sync/src/server.rs
git commit -m "feat: expose capture event timeline API"
```

---

### Task 5: Final Verification

**Files:**
- Verify only; no file changes expected.

- [ ] **Step 1: Run crate tests**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo test
cd E:\project\AEON-FLOW\aeon-sync
cargo test
```

Expected: both crates pass all tests.

- [ ] **Step 2: Run formatting**

Run:

```powershell
cd E:\project\AEON-FLOW\aeon-capture
cargo fmt
cd E:\project\AEON-FLOW\aeon-sync
cargo fmt
```

Expected: formatting completes without errors.

- [ ] **Step 3: Confirm git status**

Run:

```powershell
cd E:\project\AEON-FLOW
git status --short --branch
```

Expected: working tree is clean and current branch is `codex/feature-plateform`.

- [ ] **Step 4: Summarize the phase**

Report:

- event model added
- JSONL event log added
- capture pipeline appends one event after successful store writes
- `/api/events` and `/api/events/:id` added
- existing capture behavior preserved
- exact tests run and whether they passed

---

## Self-Review Notes

Spec coverage:

- Typed `AeonEvent` model: Task 1
- Append-only event log: Task 2
- Capture pipeline projection: Task 3
- Read-only event APIs: Task 4
- Projection, persistence, query, corrupt-line, and failure behavior tests: Tasks 1 through 4

Scope control:

- No screen OCR, keyboard hooks, HTTP proxy, audio capture, credential storage, browser injection, AI query, RocksDB, or Tantivy work appears in this plan.
- `CaptureEntry` remains intact and existing capture APIs stay in place.
- Event reads are owned by `EventLog`; server handlers do not read the JSONL file directly.
