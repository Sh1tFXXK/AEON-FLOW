pub mod apps;
pub mod bridge;
pub mod capture;
pub mod clipboard;
pub mod engine;
pub mod event;
pub mod event_log;
pub mod file;
pub mod os_activity;
pub mod platform;
pub mod screenshot;
pub mod store;

pub use aeon_store::{hex_cid, parse_cid_hex};
pub use capture::{
    CaptureEntry, CaptureKind, CaptureMetadata, CaptureSource, OsCaptureProvider, CID,
};
pub use engine::CaptureEngine;
pub use event::{AeonEvent, CaptureEvent, EventId, EventKind, EventSource};
pub use event_log::{EventLog, EventQuery};
pub use os_activity::{ForegroundWindow, InputSensitivity, OsActivity, TextCommit, WindowBounds};
pub use store::{CaptureRecord, CaptureStore};
