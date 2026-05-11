use axum::{
    body::Body,
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Multipart, Path, State,
    },
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;

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
}

#[derive(Deserialize)]
pub struct SavePayload {
    pub content: String,
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
}

#[derive(Serialize, Deserialize)]
pub struct HistoryEntry {
    pub version: u64,
    pub cid: String,
    pub modified: u64,
    pub deleted: bool,
}

#[derive(Serialize)]
pub struct DeviceStatus {
    pub name: String,
    pub online: bool,
}

pub async fn status(State(state): State<AppState>) -> Json<StatusPayload> {
    Json(StatusPayload {
        identity_short: state.identity_short,
        devices: vec![DeviceStatus {
            name: "本机".to_string(),
            online: true,
        }],
    })
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

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(|socket| handle_ws(socket, state))
}

async fn handle_ws(mut socket: WebSocket, state: AppState) {
    let mut rx = state.file_events.subscribe();
    while let Ok(event_name) = rx.recv().await {
        if socket
            .send(Message::Text(format!(
                r#"{{"type":"refresh","event":"{}","at":{}}}"#,
                event_name,
                now_ms()
            )))
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
        .route("/api/files", get(list_files))
        .route("/api/history/:filename", get(file_history))
        .route("/api/upload", post(upload_file))
        .route("/api/download/:filename", get(download_file))
        .route("/api/files/:filename", delete(delete_file))
        .route("/api/files/:filename", post(save_file))
        .with_state(state)
}

fn history_path(sync_dir: &PathBuf, name: &str) -> PathBuf {
    let dir = sync_dir.join(".aeon-history");
    dir.join(format!("{}.json", name.replace("/", "_")))
}

async fn append_history(
    sync_dir: &PathBuf,
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

fn meta_path(sync_dir: &PathBuf, name: &str) -> PathBuf {
    let dir = sync_dir.join(".aeon-meta");
    dir.join(format!("{}.json", name.replace("/", "_")))
}

async fn read_file_meta(sync_dir: &PathBuf, name: &str) -> std::io::Result<FileMeta> {
    let bytes = tokio::fs::read(meta_path(sync_dir, name)).await?;
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

async fn write_file_meta(sync_dir: &PathBuf, name: &str, meta: FileMeta) -> std::io::Result<()> {
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
