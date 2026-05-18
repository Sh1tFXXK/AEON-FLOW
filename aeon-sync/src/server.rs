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

#[derive(Serialize, Deserialize, Default, Clone)]
struct FileMeta {
    source_device: String,
    last_writer: String,
    updated_at: u64,
}

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
    pub devices: Arc<Mutex<DeviceRegistry>>,
    pub connect_urls: Vec<ConnectUrl>,
    pub relay_url: Option<String>,
    pub relay_space: String,
    pub device_name: String,
}

#[derive(Deserialize)]
pub struct SavePayload {
    pub content: String,
}

#[derive(Deserialize)]
pub struct CaptureTextPayload {
    pub text: String,
    pub title: Option<String>,
    pub source: Option<String>,
}

#[derive(Deserialize)]
pub struct CaptureWebpagePayload {
    pub url: String,
    pub title: Option<String>,
    pub source: Option<String>,
}

#[derive(Deserialize)]
pub struct EditEntryPayload {
    pub text: String,
    pub title: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct EventListParams {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct FileEntry {
    pub name: String,
    pub size: u64,
    pub size_human: String,
    pub mime: String,
    pub modified: u64,
    pub cid: String,
    pub is_dir: bool,
    pub source_device: String,
}

#[derive(Serialize)]
pub struct StatusPayload {
    pub identity_short: String,
    pub devices: Vec<DeviceStatus>,
    pub connect_urls: Vec<ConnectUrl>,
}

#[derive(Serialize, Deserialize)]
pub struct HistoryEntry {
    pub version: u64,
    pub cid: String,
    pub modified: u64,
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectUrl {
    pub id: String,
    pub label: String,
    pub url: String,
    pub kind: String,
    pub remote: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceStatus {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub kind: String,
    pub endpoint: Option<String>,
    pub last_seen_ms: Option<u64>,
    pub is_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: Option<String>,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceRegistry {
    peers: HashMap<String, PeerDevice>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceHelloPayload {
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Deserialize)]
pub struct CaptureProcessRequest {
    pub pid: u32,
    pub option_id: String,
    pub target_device: Option<String>,
}

#[derive(Serialize)]
pub struct CapturePayload {
    pub cid: String,
    pub kind: String,
    pub kind_label: String,
    pub title: String,
    pub summary: Option<String>,
    pub source: String,
    pub source_label: String,
    pub captured_at: u64,
    pub size: usize,
    pub mime: String,
    pub app_name: Option<String>,
    pub file_path: Option<String>,
    pub url: Option<String>,
    pub message_count: Option<usize>,
    pub previous_version: Option<String>,
    pub extra: HashMap<String, String>,
    pub editable: bool,
    pub raw_url: String,
}

#[derive(Serialize)]
pub struct CaptureDetailPayload {
    #[serde(flatten)]
    pub entry: CapturePayload,
    pub text: Option<String>,
}

#[derive(Serialize)]
pub struct EventPayload {
    pub id: String,
    pub ts: u64,
    pub kind: aeon_capture::EventKind,
    pub source: aeon_capture::EventSource,
    pub device: String,
    pub identity: String,
}

impl DeviceRegistry {
    pub fn upsert(&mut self, device: PeerDevice) {
        self.peers.insert(device.id.clone(), device);
    }

    pub fn list(&mut self, now: u64) -> Vec<DeviceStatus> {
        self.peers
            .retain(|_, peer| now.saturating_sub(peer.last_seen) <= DEVICE_KEEP_OFFLINE_MS);

        let mut devices: Vec<_> = self
            .peers
            .values()
            .map(|peer| {
                let age = now.saturating_sub(peer.last_seen);
                DeviceStatus {
                    id: peer.id.clone(),
                    name: peer.name.clone(),
                    online: age <= DEVICE_ONLINE_TTL_MS,
                    kind: peer.kind.clone(),
                    endpoint: peer.endpoint.clone(),
                    last_seen_ms: Some(age),
                    is_local: false,
                }
            })
            .collect();
        devices.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then(a.kind.cmp(&b.kind))
                .then(a.name.cmp(&b.name))
        });
        devices
    }
}

pub async fn status(State(state): State<AppState>) -> Json<StatusPayload> {
    let mut devices = vec![DeviceStatus {
        id: "local".to_string(),
        name: state.device_name.clone(),
        online: true,
        kind: "desktop".to_string(),
        endpoint: None,
        last_seen_ms: Some(0),
        is_local: true,
    }];
    devices.extend(state.devices.lock().await.list(now_ms()));

    Json(StatusPayload {
        identity_short: state.identity_short,
        devices,
        connect_urls: state.connect_urls.clone(),
    })
}

pub async fn device_hello(
    State(state): State<AppState>,
    Json(payload): Json<DeviceHelloPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let Some(id) = payload
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "local")
        .map(ToOwned::to_owned)
    else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Unnamed device")
        .to_string();
    let kind = payload
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let endpoint = payload
        .endpoint
        .map(|endpoint| endpoint.trim().trim_end_matches('/').to_string())
        .filter(|endpoint| !endpoint.is_empty());

    state.devices.lock().await.upsert(PeerDevice {
        id: id.clone(),
        name,
        kind,
        endpoint,
        last_seen: now_ms(),
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "id": id
    })))
}

pub async fn file_history(
    Path(filename): Path<String>,
    State(state): State<AppState>,
) -> Json<Vec<HistoryEntry>> {
    let Some(safe_name) = sanitize_filename(&filename) else {
        return Json(vec![]);
    };
    let path = history_path(&state.sync_dir, &safe_name);
    match tokio::fs::read(&path).await {
        Ok(bytes) => Json(serde_json::from_slice::<Vec<HistoryEntry>>(&bytes).unwrap_or_default()),
        Err(_) => Json(vec![]),
    }
}

pub async fn list_files(State(state): State<AppState>) -> Json<Vec<FileEntry>> {
    let mut entries = Vec::new();
    if let Ok(mut dir) = tokio::fs::read_dir(&state.sync_dir).await {
        while let Ok(Some(entry)) = dir.next_entry().await {
            let path = entry.path();
            if let Ok(meta) = entry.metadata().await {
                let name = entry.file_name().to_string_lossy().to_string();
                let size = meta.len();
                let is_dir = meta.is_dir();
                let mime = if is_dir {
                    "inode/directory".to_string()
                } else {
                    mime_guess::from_path(&path)
                        .first_or_octet_stream()
                        .to_string()
                };
                let modified = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let cid = if !is_dir {
                    match tokio::fs::read(&path).await {
                        Ok(data) => blake3::hash(&data).to_hex()[..8].to_string(),
                        Err(_) => "unknown".to_string(),
                    }
                } else {
                    "dir".to_string()
                };
                let source_device = read_file_meta(&state.sync_dir, &name)
                    .await
                    .ok()
                    .map(|m| m.source_device)
                    .unwrap_or_else(local_device_label);
                entries.push(FileEntry {
                    name,
                    size,
                    size_human: human_size(size),
                    mime,
                    modified,
                    cid,
                    is_dir,
                    source_device,
                });
            }
        }
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Json(entries)
}

pub async fn list_entries(State(state): State<AppState>) -> Json<Vec<CapturePayload>> {
    let entries = state
        .capture_engine
        .list()
        .await
        .into_iter()
        .map(capture_payload)
        .collect();
    Json(entries)
}

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

pub async fn get_entry(
    Path(cid_hex): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<CaptureDetailPayload>, StatusCode> {
    let cid = parse_cid_hex(&cid_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
    let entry = state
        .capture_engine
        .get(&cid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let record = capture_record_from_entry(&entry);
    let text = std::str::from_utf8(&entry.data).ok().map(ToOwned::to_owned);

    Ok(Json(CaptureDetailPayload {
        entry: capture_payload(record),
        text,
    }))
}

pub async fn edit_entry(
    Path(cid_hex): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<EditEntryPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let previous_cid = parse_cid_hex(&cid_hex).map_err(|_| StatusCode::BAD_REQUEST)?;
    let previous = state
        .capture_engine
        .get(&previous_cid)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    if std::str::from_utf8(&previous.data).is_err() {
        return Err(StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    let mut entry = CaptureEntry::new(
        payload.text.into_bytes(),
        previous.kind.clone(),
        CaptureSource::Manual,
    );
    if entry.cid == previous_cid {
        return Ok(Json(serde_json::json!({
            "ok": true,
            "unchanged": true,
            "cid": hex_cid(&previous_cid),
        })));
    }

    entry.meta = previous.meta.clone();
    entry.meta.previous_version = Some(previous_cid);
    entry.meta.summary = None;
    entry
        .meta
        .extra
        .insert("edited_from".to_string(), hex_cid(&previous_cid));
    entry
        .meta
        .extra
        .insert("edited_at".to_string(), now_ms().to_string());
    if let Some(title) = payload.title.filter(|title| !title.trim().is_empty()) {
        entry.meta.title = Some(title);
    }

    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "previous": hex_cid(&previous_cid),
    })))
}

pub async fn download_entry(
    Path(cid_hex): Path<String>,
    State(state): State<AppState>,
) -> Response {
    let Ok(cid) = parse_cid_hex(&cid_hex) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match state.capture_engine.raw(&cid).await {
        Ok(Some(blob)) => Response::builder()
            .header(header::CONTENT_TYPE, blob.mime)
            .header(
                header::CONTENT_DISPOSITION,
                format!("inline; filename=\"{}.bin\"", &hex_cid(&cid)[..12]),
            )
            .body(Body::from(blob.data))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

pub async fn capture_text(
    headers: HeaderMap,
    State(state): State<AppState>,
    Json(payload): Json<CaptureTextPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let source = payload.source.clone();
    let mut entry = if is_http_url(payload.text.trim()) {
        tokio::task::spawn_blocking({
            let text = payload.text.clone();
            let title = payload.title.clone();
            let source = source.clone().unwrap_or_else(|| "Shared URL".to_string());
            move || capture_webpage_url(&text, title.as_deref(), &source, "shared-url")
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::BAD_REQUEST)?
    } else {
        let mut entry = CaptureEntry::new(
            payload.text.into_bytes(),
            CaptureKind::Text,
            source_from_peer_headers(&headers, CaptureSource::Manual),
        );
        if let Some(title) = payload.title {
            entry = entry.with_title(&title);
        }
        entry
    };
    if let Some(source) = source {
        entry.meta.app_name = Some(source);
    }
    annotate_peer_metadata(&mut entry, &headers);
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true, "cid": hex_cid(&cid)})))
}

pub async fn capture_webpage(
    State(state): State<AppState>,
    Json(payload): Json<CaptureWebpagePayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let source = payload
        .source
        .unwrap_or_else(|| "Manual webpage".to_string());
    let mut entry = tokio::task::spawn_blocking(move || {
        capture_webpage_url(
            &payload.url,
            payload.title.as_deref(),
            &source,
            "manual-url",
        )
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::BAD_REQUEST)?;
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true, "cid": hex_cid(&cid)})))
}

pub async fn capture_drop(
    headers: HeaderMap,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut captured = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field.file_name().unwrap_or("dropped-content").to_string();
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        if data.is_empty() {
            continue;
        }

        let kind = aeon_capture::file::kind_from_path(std::path::Path::new(&filename), &data);
        let mut entry = CaptureEntry::new(
            data.to_vec(),
            kind,
            source_from_peer_headers(&headers, CaptureSource::DragDrop),
        )
        .with_title(&filename);
        annotate_peer_metadata(&mut entry, &headers);
        stamp_capture_identity(&mut entry, &state);
        let cid = state
            .capture_engine
            .capture(entry)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        captured.push(serde_json::json!({
            "name": filename,
            "cid": hex_cid(&cid),
        }));
    }

    Ok(Json(serde_json::json!({"captured": captured})))
}

pub async fn capture_apps(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (captured, attempts) = capture_known_apps(&state).await;
    Json(serde_json::json!({
        "captured": captured,
        "attempts": attempts,
    }))
}

pub async fn capture_processes(State(state): State<AppState>) -> Json<serde_json::Value> {
    match capture_process_inventory(&state).await {
        Ok((cid, count)) => Json(serde_json::json!({
            "ok": true,
            "cid": hex_cid(&cid),
            "captured": [hex_cid(&cid)],
            "process_count": count,
            "relay": state.relay_url.is_some(),
            "relay_space": state.relay_space.clone(),
            "message": format!("已捕获 {count} 个运行进程"),
        })),
        Err(error) => Json(serde_json::json!({"ok": false, "error": error})),
    }
}

pub async fn capture_all(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut captured = Vec::new();
    let mut errors = Vec::new();
    let process_count = match capture_process_inventory(&state).await {
        Ok((cid, count)) => {
            captured.push(hex_cid(&cid));
            count
        }
        Err(error) => {
            errors.push(error);
            0
        }
    };
    let (app_captured, attempts) = capture_known_apps(&state).await;
    captured.extend(app_captured);
    let (window_captured, window_attempts) = capture_visible_app_windows(&state).await;
    captured.extend(window_captured);

    Json(serde_json::json!({
        "ok": errors.is_empty() || !captured.is_empty(),
        "captured": captured,
        "process_count": process_count,
        "attempts": attempts,
        "window_attempts": window_attempts,
        "errors": errors,
        "relay": state.relay_url.is_some(),
        "relay_space": state.relay_space.clone(),
        "message": "全机状态已捕获",
    }))
}

async fn capture_known_apps(state: &AppState) -> (Vec<String>, Vec<serde_json::Value>) {
    let mut captured = Vec::new();
    let mut attempts = Vec::new();

    for handler in state.app_registry.handlers() {
        let app = handler.app_name().to_string();
        if !handler.is_running() {
            attempts.push(serde_json::json!({
                "app": app,
                "running": false,
                "captured": null,
                "reason": "not running",
            }));
            continue;
        }

        let mut entry = match aeon_capture::apps::capture_app_entry(handler.as_ref()) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                attempts.push(serde_json::json!({
                    "app": app,
                    "running": true,
                    "captured": null,
                    "reason": "no capturable state found",
                }));
                continue;
            }
            Err(reason) => {
                attempts.push(serde_json::json!({
                    "app": app,
                    "running": true,
                    "captured": null,
                    "reason": reason,
                }));
                continue;
            }
        };

        stamp_capture_identity(&mut entry, state);
        match state.capture_engine.capture(entry).await {
            Ok(cid) => {
                let hex = hex_cid(&cid);
                captured.push(hex.clone());
                attempts.push(serde_json::json!({
                    "app": app,
                    "running": true,
                    "captured": hex,
                    "reason": null,
                }));
            }
            Err(err) => attempts.push(serde_json::json!({
                "app": app,
                "running": true,
                "captured": null,
                "reason": format!("store failed: {err}"),
            })),
        }
    }

    (captured, attempts)
}

async fn capture_visible_app_windows(state: &AppState) -> (Vec<String>, Vec<serde_json::Value>) {
    let windows = tokio::task::spawn_blocking(aeon_capture::screenshot::list_visible_windows)
        .await
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut captured = Vec::new();
    let mut attempts = Vec::new();

    for window in windows {
        if captured.len() >= MAX_VISIBLE_APP_CAPTURES {
            break;
        }
        if !seen.insert(window.pid) {
            continue;
        }
        let process_name = crate::process::process_name(window.pid)
            .unwrap_or_else(|| format!("pid-{}", window.pid));
        if is_ignored_window_process(&process_name) {
            continue;
        }
        match capture_generic_app_state(window.pid, state).await {
            Ok(value) => {
                if let Some(cid) = value.get("cid").and_then(|cid| cid.as_str()) {
                    captured.push(cid.to_string());
                }
                attempts.push(serde_json::json!({
                    "pid": window.pid,
                    "title": window.title,
                    "process": process_name,
                    "captured": value.get("cid").and_then(|cid| cid.as_str()),
                    "screenshot": value.get("screenshot_cid").and_then(|cid| cid.as_str()),
                    "reason": null,
                }));
            }
            Err(error) => attempts.push(serde_json::json!({
                "pid": window.pid,
                "title": window.title,
                "process": process_name,
                "captured": null,
                "reason": error,
            })),
        }
    }

    (captured, attempts)
}

fn is_ignored_window_process(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "explorer.exe"
            | "searchhost.exe"
            | "startmenuexperiencehost.exe"
            | "shellexperiencehost.exe"
            | "applicationframehost.exe"
    )
}

async fn capture_process_inventory(state: &AppState) -> Result<(CID, usize), String> {
    let processes = tokio::task::spawn_blocking(crate::process::list_processes)
        .await
        .map_err(|err| err.to_string())?;
    let count = processes.len();
    let payload = serde_json::json!({
        "captured_at": now_ms(),
        "process_count": count,
        "processes": processes,
    });
    let data = serde_json::to_vec_pretty(&payload).map_err(|err| err.to_string())?;
    let mut entry = CaptureEntry::new(data, CaptureKind::ProcessState, CaptureSource::Manual)
        .with_title(&format!("全机进程清单 ({count})"))
        .with_summary(&format!("捕获 {count} 个正在运行的进程"))
        .with_app("Processes");
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "process-inventory".to_string());
    entry
        .meta
        .extra
        .insert("process_count".to_string(), count.to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok((cid, count))
}

pub async fn list_process_entries() -> Json<Vec<crate::process::ProcessInfo>> {
    let processes = tokio::task::spawn_blocking(crate::process::list_processes)
        .await
        .unwrap_or_default();
    Json(processes)
}

pub async fn list_vm_entries() -> Json<Vec<AeonVmInfo>> {
    let vms = tokio::task::spawn_blocking(|| list_recent_vms(240))
        .await
        .unwrap_or_default();
    Json(vms)
}

pub async fn capture_process(
    Path(pid): Path<u32>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let capture = ProcessStateCapture { pid };
    let Some(mut entry) = capture.capture() else {
        return Err(StatusCode::NOT_FOUND);
    };
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true, "cid": hex_cid(&cid)})))
}

pub async fn capture_process_option(
    State(state): State<AppState>,
    Json(req): Json<CaptureProcessRequest>,
) -> Json<serde_json::Value> {
    Json(execute_capture_option(req, &state).await)
}

async fn execute_capture_option(req: CaptureProcessRequest, state: &AppState) -> serde_json::Value {
    let result = match req.option_id.as_str() {
        id if id.starts_with("screenshot") => capture_window_screenshot(req.pid, state).await,
        "claude_conversation" => {
            capture_app_entry(ClaudeDesktopCapture, state, "对话已捕获到 AEON").await
        }
        "vscode_workspace" | "vscode_current_file" => {
            capture_app_entry(VSCodeCapture, state, "VS Code 工作区已捕获").await
        }
        "browser_tab" => capture_browser_tab(req.pid, state).await,
        "browser_pages" => capture_browser_pages_option(req.pid, state).await,
        "browser_bookmarks" => capture_chrome_bookmarks(state).await,
        "terminal_state" => capture_terminal_state_option(state).await,
        "obsidian_vault" => {
            capture_process_metadata(req.pid, state, Some("Obsidian 笔记库线索")).await
        }
        "metadata" => capture_process_metadata(req.pid, state, None).await,
        id if id.starts_with("metadata_") => capture_process_metadata(req.pid, state, None).await,
        id if id.starts_with("app_state_") => capture_generic_app_state(req.pid, state).await,
        id if id.starts_with("snapshot_") => {
            let vm_id = id.trim_start_matches("snapshot_");
            capture_vm_action(vm_id, state, None, "VM 快照已捕获").await
        }
        id if id.starts_with("migrate_") => {
            let vm_id = id.trim_start_matches("migrate_");
            let target = req.target_device.as_deref().unwrap_or("aeon-relay");
            capture_vm_action(vm_id, state, Some(target), "已生成迁移快照").await
        }
        id if id.starts_with("pause_") => {
            let vm_id = id.trim_start_matches("pause_");
            match set_vm_status(vm_id, "paused") {
                Ok(_) => capture_vm_action(vm_id, state, None, "VM 已暂停并捕获快照").await,
                Err(err) => Err(err),
            }
        }
        _ => Err("未知操作".to_string()),
    };

    match result {
        Ok(value) => value,
        Err(error) => serde_json::json!({"ok": false, "error": error}),
    }
}

async fn capture_app_entry<T>(
    capture: T,
    state: &AppState,
    message: &str,
) -> Result<serde_json::Value, String>
where
    T: AppCapture,
{
    let mut entry = aeon_capture::apps::capture_app_entry(&capture)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "没有找到可捕获的应用状态".to_string())?;
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": message,
    }))
}

async fn capture_browser_tab(pid: u32, state: &AppState) -> Result<serde_json::Value, String> {
    let name = crate::process::process_name(pid).unwrap_or_default();
    let browser = browser_name_from_process(&name);
    capture_app_entry(
        BrowserCapture {
            browser: browser.to_string(),
        },
        state,
        "浏览器标签页已捕获",
    )
    .await
}

async fn capture_browser_pages_option(
    pid: u32,
    state: &AppState,
) -> Result<serde_json::Value, String> {
    let name = crate::process::process_name(pid).unwrap_or_default();
    let browser = browser_name_from_process(&name);
    let mut entry = tokio::task::spawn_blocking(move || capture_browser_pages(browser, 30))
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "没有找到浏览器页面历史".to_string())?;
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "浏览器网页清单已捕获",
    }))
}

fn browser_name_from_process(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("firefox") {
        "Firefox"
    } else if lower.contains("edge") || lower.contains("msedge") {
        "Edge"
    } else {
        "Chrome"
    }
}

async fn capture_terminal_state_option(state: &AppState) -> Result<serde_json::Value, String> {
    let mut entry = tokio::task::spawn_blocking(capture_terminal_state)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "没有找到终端历史或运行中的终端".to_string())?;
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "终端状态已捕获",
    }))
}

async fn capture_chrome_bookmarks(state: &AppState) -> Result<serde_json::Value, String> {
    let path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "找不到 LOCALAPPDATA".to_string())?
        .join("Google")
        .join("Chrome")
        .join("User Data")
        .join("Default")
        .join("Bookmarks");
    let data = tokio::fs::read(&path)
        .await
        .map_err(|err| format!("读取书签失败: {err}"))?;
    let mut entry = CaptureEntry::new(
        data,
        CaptureKind::Document {
            format: "json".to_string(),
        },
        CaptureSource::AppApi {
            app: "Chrome".to_string(),
        },
    )
    .with_title("Chrome 书签");
    entry.meta.file_path = Some(path.to_string_lossy().to_string());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "chrome-bookmarks".to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "Chrome 书签已捕获",
    }))
}

async fn capture_window_screenshot(
    pid: u32,
    state: &AppState,
) -> Result<serde_json::Value, String> {
    let (data, width, height) = tokio::task::spawn_blocking(move || {
        aeon_capture::screenshot::capture_window_screenshot_bytes(pid)
    })
    .await
    .map_err(|err| err.to_string())??;
    let process_name =
        crate::process::process_name(pid).unwrap_or_else(|| format!("process-{pid}"));
    let mut entry = CaptureEntry::new(
        data,
        CaptureKind::Image {
            width,
            height,
            format: "png".to_string(),
        },
        CaptureSource::Screenshot,
    )
    .with_title(&format!("{process_name} 截图"));
    entry.meta.extra.insert("pid".to_string(), pid.to_string());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "window-screenshot".to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "截图已捕获",
    }))
}

async fn capture_generic_app_state(
    pid: u32,
    state: &AppState,
) -> Result<serde_json::Value, String> {
    let metadata =
        crate::process::process_metadata(pid).ok_or_else(|| "process not found".to_string())?;
    let process_name = metadata
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("process")
        .to_string();
    let windows = tokio::task::spawn_blocking(aeon_capture::screenshot::list_visible_windows)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|window| window.pid == pid)
        .collect::<Vec<_>>();

    let screenshot = capture_window_screenshot(pid, state).await.ok();
    let screenshot_cid = screenshot
        .as_ref()
        .and_then(|value| value.get("cid"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);

    let payload = serde_json::json!({
        "capture_mode": "generic-application-state",
        "captured_at": now_ms(),
        "process": metadata,
        "windows": windows,
        "screenshot_cid": screenshot_cid,
    });
    let data = serde_json::to_vec_pretty(&payload).map_err(|err| err.to_string())?;
    let mut entry = CaptureEntry::new(data, CaptureKind::ProcessState, CaptureSource::Manual)
        .with_title(&format!("{process_name} application state"))
        .with_summary(&format!(
            "Captured process metadata{} for PID {pid}",
            if screenshot_cid.is_some() {
                " and window screenshot"
            } else {
                ""
            }
        ))
        .with_app(&process_name);
    entry.meta.extra.insert("pid".to_string(), pid.to_string());
    entry.meta.extra.insert(
        "capture_mode".to_string(),
        "generic-application-state".to_string(),
    );
    if let Some(cid) = &screenshot_cid {
        entry
            .meta
            .extra
            .insert("screenshot_cid".to_string(), cid.clone());
    }
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "screenshot_cid": screenshot_cid,
        "message": "应用状态已捕获",
    }))
}

async fn capture_process_metadata(
    pid: u32,
    state: &AppState,
    title: Option<&str>,
) -> Result<serde_json::Value, String> {
    let metadata = crate::process::process_metadata(pid).ok_or_else(|| "进程不存在".to_string())?;
    let data = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
    let name = metadata
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("process");
    let title = title
        .map(str::to_string)
        .unwrap_or_else(|| format!("{name} 进程信息"));
    let mut entry = CaptureEntry::new(data, CaptureKind::ProcessState, CaptureSource::Manual)
        .with_title(&title)
        .with_app("Process");
    entry.meta.extra.insert("pid".to_string(), pid.to_string());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "process-metadata".to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "进程信息已捕获",
    }))
}

async fn capture_vm_action(
    vm_id: &str,
    state: &AppState,
    target_device: Option<&str>,
    message: &str,
) -> Result<serde_json::Value, String> {
    let mut entry = capture_vm_snapshot(vm_id)?;
    let transfer_target = target_device.filter(|target| !target.trim().is_empty());
    if transfer_target.is_some() && state.relay_url.is_none() {
        return Err(
            "AEON Relay is not enabled; start with scripts\\aeon.ps1 to transfer VM snapshots"
                .to_string(),
        );
    }
    if let Some(target) = transfer_target {
        entry.meta.title = Some(format!("VM 迁移快照 {vm_id} -> {target}"));
        entry
            .meta
            .extra
            .insert("migration_target".to_string(), target.to_string());
        entry
            .meta
            .extra
            .insert("transfer_mode".to_string(), "aeon-relay".to_string());
    }
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": message,
        "vm_id": vm_id,
        "target": transfer_target,
        "relay": state.relay_url.is_some(),
        "relay_space": state.relay_space.clone(),
    }))
}

pub async fn capture_vm(
    Path(vm_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut entry = capture_vm_snapshot(&vm_id).map_err(|err| {
        tracing::warn!("capture vm {vm_id} failed: {err}");
        StatusCode::NOT_FOUND
    })?;
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true, "cid": hex_cid(&cid)})))
}

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut file_rx = state.file_events.subscribe();
    let mut capture_rx = state.capture_engine.subscribe();

    loop {
        let payload = tokio::select! {
            Ok(event_name) = file_rx.recv() => serde_json::json!({
                "type": "refresh",
                "event": event_name,
                "at": now_ms(),
            }),
            Ok(entry) = capture_rx.recv() => {
                let record = capture_record_from_entry(&entry);
                serde_json::json!({
                    "type": "capture",
                    "entry": capture_payload(record),
                    "at": now_ms(),
                })
            },
            else => break,
        };

        if socket
            .send(Message::Text(payload.to_string()))
            .await
            .is_err()
        {
            break;
        }
    }
}

pub async fn download_file(
    Path(filename): Path<String>,
    State(state): State<AppState>,
) -> Response {
    let Some(safe_name) = sanitize_filename(&filename) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let path = state.sync_dir.join(&safe_name);
    if !path.starts_with(&state.sync_dir) {
        return StatusCode::FORBIDDEN.into_response();
    }
    match tokio::fs::File::open(&path).await {
        Err(_) => StatusCode::NOT_FOUND.into_response(),
        Ok(file) => {
            let mime = mime_guess::from_path(&path)
                .first_or_octet_stream()
                .to_string();
            let stream = ReaderStream::new(file);
            let body = Body::from_stream(stream);
            Response::builder()
                .header(header::CONTENT_TYPE, mime)
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", safe_name),
                )
                .body(body)
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

pub async fn upload_file(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut uploaded = Vec::new();
    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field.file_name().unwrap_or("unknown").to_string();
        let Some(safe_name) = sanitize_filename(&filename) else {
            continue;
        };
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        let dest = state.sync_dir.join(&safe_name);
        tokio::fs::write(&dest, &data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let mime = mime_guess::from_path(&dest)
            .first_or_octet_stream()
            .to_string();
        let cid = blake3::hash(&data).to_hex()[..8].to_string();
        uploaded.push(serde_json::json!({"name":safe_name,"size":data.len(),"cid":cid}));
        tracing::info!("Uploaded: {} ({} bytes)", filename, data.len());
        let _ = append_history(&state.sync_dir, &safe_name, cid, false).await;
        let _ = write_file_meta(
            &state.sync_dir,
            &safe_name,
            FileMeta {
                source_device: local_device_label(),
                last_writer: local_device_label(),
                updated_at: now_ms(),
            },
        )
        .await;
        let mut entry = CaptureEntry::new(
            data.to_vec(),
            capture_kind_for_file(&safe_name, &mime, &data),
            CaptureSource::Manual,
        )
        .with_title(&safe_name);
        entry.meta.file_path = Some(dest.to_string_lossy().to_string());
        stamp_capture_identity(&mut entry, &state);
        let _ = state.capture_engine.capture(entry).await;
    }
    let _ = state.file_events.send("upload".to_string());
    Ok(Json(serde_json::json!({"uploaded":uploaded})))
}

pub async fn delete_file(
    Path(filename): Path<String>,
    State(state): State<AppState>,
) -> StatusCode {
    let Some(safe_name) = sanitize_filename(&filename) else {
        return StatusCode::BAD_REQUEST;
    };
    let path = state.sync_dir.join(&safe_name);
    if !path.starts_with(&state.sync_dir) {
        return StatusCode::FORBIDDEN;
    }
    let cid = tokio::fs::read(&path)
        .await
        .ok()
        .map(|d| blake3::hash(&d).to_hex()[..8].to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let status = match tokio::fs::remove_file(&path).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::NOT_FOUND,
    };
    if status == StatusCode::NO_CONTENT {
        let _ = append_history(&state.sync_dir, &safe_name, cid, true).await;
        let _ = state.file_events.send("delete".to_string());
    }
    status
}

pub async fn save_file(
    Path(filename): Path<String>,
    State(state): State<AppState>,
    Json(payload): Json<SavePayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let Some(safe_name) = sanitize_filename(&filename) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let path = state.sync_dir.join(&safe_name);
    if !path.starts_with(&state.sync_dir) {
        return Err(StatusCode::FORBIDDEN);
    }
    tokio::fs::write(&path, payload.content.as_bytes())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let cid = blake3::hash(payload.content.as_bytes()).to_hex()[..8].to_string();
    let _ = append_history(&state.sync_dir, &safe_name, cid.clone(), false).await;
    let existing_source = read_file_meta(&state.sync_dir, &safe_name)
        .await
        .ok()
        .map(|m| m.source_device)
        .unwrap_or_else(local_device_label);
    let _ = write_file_meta(
        &state.sync_dir,
        &safe_name,
        FileMeta {
            source_device: existing_source,
            last_writer: local_device_label(),
            updated_at: now_ms(),
        },
    )
    .await;
    let mut entry = CaptureEntry::new(
        payload.content.as_bytes().to_vec(),
        capture_kind_for_file(&safe_name, "text/plain", payload.content.as_bytes()),
        CaptureSource::FileWatch {
            path: path.to_string_lossy().to_string(),
        },
    )
    .with_title(&safe_name);
    entry.meta.file_path = Some(path.to_string_lossy().to_string());
    stamp_capture_identity(&mut entry, &state);
    let _ = state.capture_engine.capture(entry).await;
    let _ = state.file_events.send("save".to_string());
    Ok(Json(serde_json::json!({"ok":true,"cid":cid})))
}

pub async fn index_page() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/ws", get(ws_handler))
        .route("/api/status", get(status))
        .route("/api/devices/hello", post(device_hello))
        .route("/api/events", get(list_events))
        .route("/api/events/:id", get(get_event))
        .route("/api/entries", get(list_entries))
        .route("/api/entry/:cid", get(get_entry))
        .route("/api/entry/:cid/edit", post(edit_entry))
        .route("/api/entry/:cid/raw", get(download_entry))
        .route("/api/processes", get(list_process_entries))
        .route("/api/vms", get(list_vm_entries))
        .route("/api/capture/text", post(capture_text))
        .route("/api/capture/webpage", post(capture_webpage))
        .route("/api/capture/drop", post(capture_drop))
        .route("/api/bridge/sms", post(crate::bridge::capture_sms))
        .route("/api/bridge/email", post(crate::bridge::capture_email))
        .route("/api/capture/apps", post(capture_apps))
        .route("/api/capture/processes", post(capture_processes))
        .route("/api/capture/all", post(capture_all))
        .route("/api/capture-process", post(capture_process_option))
        .route("/api/capture/process/:pid", post(capture_process))
        .route("/api/capture/vm/:vm_id", post(capture_vm))
        .route("/api/files", get(list_files))
        .route("/api/history/:filename", get(file_history))
        .route("/api/upload", post(upload_file))
        .route("/api/download/:filename", get(download_file))
        .route("/api/files/:filename", delete(delete_file))
        .route("/api/files/:filename", post(save_file))
        .layer(DefaultBodyLimit::max(MAX_CAPTURE_UPLOAD_BYTES))
        .with_state(state)
}

fn capture_payload(record: CaptureRecord) -> CapturePayload {
    let cid = hex_cid(&record.cid);
    CapturePayload {
        raw_url: format!("/api/entry/{cid}/raw"),
        cid,
        kind: record.kind.key().to_string(),
        kind_label: kind_label(&record.kind).to_string(),
        title: record
            .meta
            .title
            .clone()
            .unwrap_or_else(|| fallback_title(&record)),
        summary: record.meta.summary.clone(),
        source: source_key(&record.source).to_string(),
        source_label: source_label(&record.source),
        captured_at: record.captured_at,
        size: record.size,
        mime: record.mime,
        app_name: record.meta.app_name.clone(),
        file_path: record.meta.file_path.clone(),
        url: record.meta.url.clone(),
        message_count: record.meta.message_count,
        previous_version: record.meta.previous_version.map(|cid| hex_cid(&cid)),
        extra: record.meta.extra.clone(),
        editable: is_editable_kind(&record.kind),
    }
}

fn event_payload(event: AeonEvent) -> EventPayload {
    EventPayload {
        id: event.id.to_hex(),
        ts: event.ts,
        kind: event.kind,
        source: event.source,
        device: hex_bytes_local(&event.device),
        identity: hex_bytes_local(&event.identity),
    }
}

fn hex_bytes_local(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn capture_record_from_entry(entry: &CaptureEntry) -> CaptureRecord {
    CaptureRecord {
        cid: entry.cid,
        kind: entry.kind.clone(),
        meta: entry.meta.clone(),
        source: entry.source.clone(),
        captured_at: entry.captured_at,
        by: entry.by,
        device: entry.device,
        size: entry.data.len(),
        mime: entry.mime(),
    }
}

fn fallback_title(record: &CaptureRecord) -> String {
    match &record.kind {
        CaptureKind::Conversation => {
            format!("对话（{} 条消息）", record.meta.message_count.unwrap_or(0))
        }
        CaptureKind::Code { language } => format!("{language} 代码"),
        CaptureKind::Image { width, height, .. } => format!("图片 {width}x{height}"),
        CaptureKind::Webpage => record
            .meta
            .url
            .clone()
            .unwrap_or_else(|| "网页".to_string()),
        CaptureKind::Clipboard => "剪贴板".to_string(),
        CaptureKind::Text => "文本".to_string(),
        CaptureKind::Document { format } => format!("{format} 文档"),
        CaptureKind::ProcessState => "进程状态".to_string(),
        CaptureKind::VmSnapshot => "AEON VM 快照".to_string(),
        CaptureKind::Blob { .. } => "二进制内容".to_string(),
    }
}

fn kind_label(kind: &CaptureKind) -> &'static str {
    match kind {
        CaptureKind::Conversation => "对话",
        CaptureKind::Code { .. } => "代码",
        CaptureKind::Text => "文本",
        CaptureKind::Image { .. } => "图片",
        CaptureKind::Webpage => "网页",
        CaptureKind::Document { .. } => "文档",
        CaptureKind::ProcessState => "进程",
        CaptureKind::VmSnapshot => "VM 快照",
        CaptureKind::Clipboard => "剪贴板",
        CaptureKind::Blob { .. } => "文件",
    }
}

fn is_editable_kind(kind: &CaptureKind) -> bool {
    matches!(
        kind,
        CaptureKind::Conversation
            | CaptureKind::Code { .. }
            | CaptureKind::Text
            | CaptureKind::Webpage
            | CaptureKind::ProcessState
            | CaptureKind::Clipboard
    )
}

fn source_key(source: &CaptureSource) -> &'static str {
    match source {
        CaptureSource::DragDrop => "DragDrop",
        CaptureSource::Clipboard => "Clipboard",
        CaptureSource::Screenshot => "Screenshot",
        CaptureSource::FileWatch { .. } => "FileWatch",
        CaptureSource::AppApi { .. } => "AppApi",
        CaptureSource::ShareMenu => "ShareMenu",
        CaptureSource::Manual => "Manual",
        CaptureSource::PeerSync { .. } => "PeerSync",
    }
}

fn source_label(source: &CaptureSource) -> String {
    match source {
        CaptureSource::DragDrop => "拖拽".to_string(),
        CaptureSource::Clipboard => "剪贴板".to_string(),
        CaptureSource::Screenshot => "截图".to_string(),
        CaptureSource::FileWatch { path } => format!("文件监控: {path}"),
        CaptureSource::AppApi { app } => app.clone(),
        CaptureSource::ShareMenu => "分享菜单".to_string(),
        CaptureSource::Manual => "手动捕获".to_string(),
        CaptureSource::PeerSync { device_name } => format!("设备同步: {device_name}"),
    }
}

fn source_from_peer_headers(headers: &HeaderMap, default: CaptureSource) -> CaptureSource {
    match header_value(headers, "x-aeon-device-kind")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "android" => CaptureSource::ShareMenu,
        "" => default,
        _ => header_value(headers, "x-aeon-device-name")
            .map(|device_name| CaptureSource::PeerSync { device_name })
            .unwrap_or(default),
    }
}

fn annotate_peer_metadata(entry: &mut CaptureEntry, headers: &HeaderMap) {
    if let Some(device_id) = header_value(headers, "x-aeon-device-id") {
        entry
            .meta
            .extra
            .insert("source_device_id".to_string(), device_id);
    }
    if let Some(device_name) = header_value(headers, "x-aeon-device-name") {
        entry
            .meta
            .extra
            .insert("source_device_name".to_string(), device_name.clone());
        if entry.meta.app_name.is_none() {
            entry.meta.app_name = Some(device_name);
        }
    }
    if let Some(device_kind) = header_value(headers, "x-aeon-device-kind") {
        entry
            .meta
            .extra
            .insert("source_device_kind".to_string(), device_kind.clone());
        if entry.meta.app_name.is_none() && device_kind.eq_ignore_ascii_case("android") {
            entry.meta.app_name = Some("Android".to_string());
        }
    }
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

fn stamp_capture_identity(entry: &mut CaptureEntry, state: &AppState) {
    entry.by = state.identity_id;
    entry.device = state.device_id;
}

fn capture_kind_for_file(name: &str, mime: &str, data: &[u8]) -> CaptureKind {
    let lower = name.to_ascii_lowercase();
    if mime.starts_with("image/") {
        let format = lower
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_string())
            .unwrap_or_else(|| mime.trim_start_matches("image/").to_string());
        let (width, height) = aeon_capture::screenshot::image_dimensions(data).unwrap_or((0, 0));
        return CaptureKind::Image {
            width,
            height,
            format,
        };
    }

    if let Some(language) = language_from_filename(&lower) {
        return CaptureKind::Code {
            language: language.to_string(),
        };
    }

    if mime.starts_with("text/") {
        if let Ok(text) = std::str::from_utf8(data) {
            return aeon_capture::clipboard::detect_text_kind(text);
        }
        return CaptureKind::Text;
    }

    if lower.ends_with(".pdf") {
        return CaptureKind::Document {
            format: "pdf".to_string(),
        };
    }
    if lower.ends_with(".docx") || lower.ends_with(".doc") {
        return CaptureKind::Document {
            format: "word".to_string(),
        };
    }

    CaptureKind::Blob {
        mime: mime.to_string(),
    }
}

fn language_from_filename(name: &str) -> Option<&'static str> {
    if name.ends_with(".rs") {
        Some("Rust")
    } else if name.ends_with(".py") {
        Some("Python")
    } else if name.ends_with(".js")
        || name.ends_with(".ts")
        || name.ends_with(".jsx")
        || name.ends_with(".tsx")
    {
        Some("JavaScript")
    } else if name.ends_with(".java") {
        Some("Java")
    } else if name.ends_with(".go") {
        Some("Go")
    } else {
        None
    }
}

fn history_path(sync_dir: &FsPath, name: &str) -> PathBuf {
    let dir = sync_dir.join(".aeon-history");
    dir.join(format!("{}.json", name.replace("/", "_")))
}

async fn append_history(
    sync_dir: &FsPath,
    name: &str,
    cid: String,
    deleted: bool,
) -> std::io::Result<()> {
    let path = history_path(sync_dir, name);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut entries: Vec<HistoryEntry> = match tokio::fs::read(&path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    if let Some(last) = entries.last() {
        if last.cid == cid && last.deleted == deleted {
            return Ok(());
        }
    }
    let version = entries.last().map(|x| x.version + 1).unwrap_or(1);
    let modified = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    entries.push(HistoryEntry {
        version,
        cid,
        modified,
        deleted,
    });
    let bytes = serde_json::to_vec(&entries).unwrap_or_default();
    tokio::fs::write(path, bytes).await
}

fn meta_path(sync_dir: &FsPath, name: &str) -> PathBuf {
    let dir = sync_dir.join(".aeon-meta");
    dir.join(format!("{}.json", name.replace("/", "_")))
}

async fn read_file_meta(sync_dir: &FsPath, name: &str) -> std::io::Result<FileMeta> {
    let bytes = tokio::fs::read(meta_path(sync_dir, name)).await?;
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn write_file_meta(sync_dir: &FsPath, name: &str, meta: FileMeta) -> std::io::Result<()> {
    let path = meta_path(sync_dir, name);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let bytes = serde_json::to_vec(&meta).unwrap_or_default();
    tokio::fs::write(path, bytes).await
}

fn local_device_label() -> String {
    std::env::var("AEON_DEVICE_NAME")
        .ok()
        .filter(|x| !x.trim().is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "本机".to_string())
}

fn human_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}
fn sanitize_filename(input: &str) -> Option<String> {
    let path = PathBuf::from(input);
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    path.file_name().and_then(|s| {
        let name = s.to_string_lossy().trim().to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    })
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_registry_reports_online_and_offline_peers() {
        let now = 1_000_000;
        let mut registry = DeviceRegistry::default();
        registry.upsert(PeerDevice {
            id: "android-1".to_string(),
            name: "Android Phone".to_string(),
            kind: "android".to_string(),
            endpoint: None,
            last_seen: now - 1_000,
        });
        registry.upsert(PeerDevice {
            id: "tablet-1".to_string(),
            name: "Tablet".to_string(),
            kind: "android".to_string(),
            endpoint: None,
            last_seen: now - DEVICE_ONLINE_TTL_MS - 1,
        });

        let devices = registry.list(now);
        let phone = devices.iter().find(|d| d.id == "android-1").unwrap();
        let tablet = devices.iter().find(|d| d.id == "tablet-1").unwrap();

        assert!(phone.online);
        assert!(!phone.is_local);
        assert!(!tablet.online);
    }

    #[test]
    fn device_registry_drops_very_old_peers() {
        let now = 1_000_000;
        let mut registry = DeviceRegistry::default();
        registry.upsert(PeerDevice {
            id: "old-phone".to_string(),
            name: "Old Phone".to_string(),
            kind: "android".to_string(),
            endpoint: None,
            last_seen: now - DEVICE_KEEP_OFFLINE_MS - 1,
        });

        assert!(registry.list(now).is_empty());
    }

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
}
