use aeon_capture::{
    apps::{
        capture_browser_pages, capture_terminal_state, capture_vm_snapshot, capture_webpage_url,
        list_recent_vms, set_vm_status, AeonVmInfo, AppCapture, AppCaptureRegistry, BrowserCapture,
        ClaudeDesktopCapture, ProcessStateCapture, VSCodeCapture,
    },
    hex_cid, parse_cid_hex, AeonEvent, CaptureEngine, CaptureEntry, CaptureKind, CaptureRecord,
    CaptureSource, EventId, EventLog, EventQuery, CID,
};
use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        DefaultBodyLimit, Multipart, Path, Query, State,
    },
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path as FsPath, PathBuf};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};
use tokio_util::io::ReaderStream;

const DEVICE_ONLINE_TTL_MS: u64 = 120_000;
const DEVICE_KEEP_OFFLINE_MS: u64 = 10 * 60_000;
const MAX_CAPTURE_UPLOAD_BYTES: usize = 1024 * 1024 * 1024;
const MAX_VISIBLE_APP_CAPTURES: usize = 16;

mod devices;
mod entries;
mod files;
mod ingest;
mod process_capture;
mod process_helpers;
mod shared;
mod ws;

pub use devices::{ConnectUrl, DeviceRegistry};

#[derive(Clone)]
pub struct AppState {
    pub sync_dir: PathBuf,
    pub file_events: broadcast::Sender<String>,
    pub identity_short: String,
    pub identity_id: [u8; 32],
    pub device_id: [u8; 16],
    pub capture_engine: Arc<CaptureEngine>,
    pub event_log: Arc<Mutex<EventLog>>,
    pub app_registry: Arc<AppCaptureRegistry>,
    pub operation_context: Arc<Mutex<crate::operation_context::ContextStore>>,
    pub account_profiles: Arc<Mutex<crate::account_profiles::AccountProfileStore>>,
    pub credential_vault: Arc<Mutex<crate::vault::CredentialVaultStore>>,
    pub vault_sessions: Arc<Mutex<crate::vault::CredentialUnlockSessions>>,
    pub email_sync: Arc<Mutex<crate::email_sync::EmailSyncStore>>,
    pub query_planner: Option<crate::query::QueryPlannerConfig>,
    pub verification_codes: Arc<Mutex<crate::bridge::VerificationCodeInbox>>,
    pub devices: Arc<Mutex<DeviceRegistry>>,
    pub connect_urls: Vec<ConnectUrl>,
    pub relay_url: Option<String>,
    pub relay_space: String,
    pub device_name: String,
}

pub async fn index_page() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/ws", get(ws::ws_handler))
        .route("/api/status", get(devices::status))
        .route("/api/devices/hello", post(devices::device_hello))
        .route("/api/events", get(entries::list_events))
        .route("/api/events/:id", get(entries::get_event))
        .route("/api/context", get(crate::operation_context::get_context))
        .route(
            "/api/context/task",
            post(crate::operation_context::set_task),
        )
        .route(
            "/api/context/clipboard",
            post(crate::operation_context::set_clipboard),
        )
        .route(
            "/api/context/scratch",
            post(crate::operation_context::set_scratch),
        )
        .route(
            "/api/context/ai-session",
            post(crate::operation_context::upsert_ai_session),
        )
        .route(
            "/api/accounts",
            get(crate::account_profiles::list_accounts)
                .post(crate::account_profiles::upsert_account),
        )
        .route(
            "/api/accounts/:id/browser-plan",
            post(crate::account_profiles::browser_launch_plan),
        )
        .route(
            "/api/vault/entries",
            get(crate::vault::list_entries).post(crate::vault::add_entry),
        )
        .route("/api/vault/totp/:id", post(crate::vault::totp_code))
        .route("/api/vault/unlock", post(crate::vault::unlock_vault))
        .route("/api/vault/fill", post(crate::vault::credential_fill))
        .route(
            "/api/email/accounts",
            get(crate::email_sync::list_email_accounts)
                .post(crate::email_sync::upsert_email_account),
        )
        .route(
            "/api/email/accounts/:id/import",
            post(crate::email_sync::import_email_messages),
        )
        .route(
            "/api/email/accounts/:id/sync",
            post(crate::email_sync::sync_email_account),
        )
        .route("/api/query", post(crate::query::query))
        .route(
            "/api/query/structured",
            post(crate::query::query_structured),
        )
        .route("/api/entries", get(entries::list_entries))
        .route("/api/entry/:cid", get(entries::get_entry))
        .route("/api/entry/:cid/edit", post(entries::edit_entry))
        .route("/api/entry/:cid/raw", get(entries::download_entry))
        .route("/api/processes", get(process_capture::list_process_entries))
        .route("/api/vms", get(process_capture::list_vm_entries))
        .route("/api/capture/text", post(ingest::capture_text))
        .route("/api/capture/webpage", post(ingest::capture_webpage))
        .route("/api/capture/drop", post(ingest::capture_drop))
        .route("/api/bridge/sms", post(crate::bridge::capture_sms))
        .route("/api/bridge/email", post(crate::bridge::capture_email))
        .route(
            "/api/bridge/browser-page",
            post(crate::bridge::capture_browser_page),
        )
        .route(
            "/api/bridge/verification-code/latest",
            get(crate::bridge::latest_verification_code),
        )
        .route("/api/capture/apps", post(process_capture::capture_apps))
        .route(
            "/api/capture/processes",
            post(process_capture::capture_processes),
        )
        .route("/api/capture/all", post(process_capture::capture_all))
        .route(
            "/api/capture-process",
            post(process_capture::capture_process_option),
        )
        .route(
            "/api/capture/process/:pid",
            post(process_capture::capture_process),
        )
        .route("/api/capture/vm/:vm_id", post(process_helpers::capture_vm))
        .route("/api/files", get(files::list_files))
        .route("/api/history/:filename", get(files::file_history))
        .route("/api/upload", post(files::upload_file))
        .route("/api/download/:filename", get(files::download_file))
        .route("/api/files/:filename", delete(files::delete_file))
        .route("/api/files/:filename", post(files::save_file))
        .layer(DefaultBodyLimit::max(MAX_CAPTURE_UPLOAD_BYTES))
        .with_state(state)
}
