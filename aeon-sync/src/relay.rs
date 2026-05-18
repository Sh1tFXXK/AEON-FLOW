use aeon_capture::{hex_cid, CaptureEngine, CaptureEntry, CaptureKind, CaptureSource};
use axum::{
    extract::{DefaultBodyLimit, Multipart, Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use tower_http::cors::CorsLayer;

const MAX_RELAY_UPLOAD_BYTES: usize = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayItem {
    pub id: String,
    pub space: String,
    pub from_device_id: String,
    pub from_device_name: String,
    pub from_kind: String,
    pub kind: String,
    pub title: Option<String>,
    pub source: Option<String>,
    pub filename: Option<String>,
    pub mime: String,
    pub cid: String,
    pub data_base64: String,
    pub captured_at: u64,
}

#[derive(Debug, Clone)]
pub struct RelayStore {
    root: PathBuf,
}

#[derive(Clone)]
struct RelayState {
    store: RelayStore,
    default_space: String,
}

#[derive(Debug, Clone)]
struct PeerHeaders {
    device_id: String,
    device_name: String,
    device_kind: String,
}

#[derive(Debug, Deserialize)]
struct RelayTextPayload {
    text: String,
    title: Option<String>,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RelayDeviceHello {
    id: Option<String>,
    name: Option<String>,
    kind: Option<String>,
    endpoint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RelayPullQuery {
    space: Option<String>,
    after: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RelayPushPayload {
    item: RelayItem,
}

#[derive(Debug, Deserialize)]
struct RelayPullResponse {
    items: Vec<RelayItem>,
}

#[derive(Debug, Deserialize)]
pub struct RelayPushResponse {
    pub ok: bool,
    pub relay_id: Option<String>,
    pub cid: Option<String>,
}

#[derive(Clone)]
pub struct RelayPullConfig {
    pub url: String,
    pub space: String,
    pub device_id: String,
    pub device_name: String,
    pub cursor_path: PathBuf,
    pub capture_engine: Arc<CaptureEngine>,
}

#[derive(Clone)]
pub struct RelayPushConfig {
    pub url: String,
    pub space: String,
    pub device_id: String,
    pub device_name: String,
    pub device_kind: String,
    pub capture_engine: Arc<CaptureEngine>,
}

impl RelayStore {
    pub fn new(root: PathBuf) -> Self {
        RelayStore { root }
    }

    pub fn push(&self, mut item: RelayItem) -> io::Result<RelayItem> {
        if item.captured_at == 0 {
            item.captured_at = now_ms();
        }
        if item.cid.trim().is_empty() {
            if let Ok(data) = BASE64.decode(&item.data_base64) {
                item.cid = blake3::hash(&data).to_hex().to_string();
            }
        }
        if item.id.trim().is_empty() {
            item.id = relay_id(item.captured_at, &item.cid, &item.from_device_id);
        } else {
            item.id = sanitize_key(&item.id, "item");
        }
        item.space = sanitize_key(&item.space, "default");
        if item.mime.trim().is_empty() {
            item.mime = "application/octet-stream".to_string();
        }

        let dir = self.space_dir(&item.space);
        fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{}.json", item.id));
        let bytes = serde_json::to_vec_pretty(&item)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        fs::write(path, bytes)?;
        Ok(item)
    }

    pub fn pull(
        &self,
        space: &str,
        after: Option<&str>,
        limit: usize,
    ) -> io::Result<Vec<RelayItem>> {
        let dir = self.space_dir(space);
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut items = Vec::new();
        for entry in fs::read_dir(dir)? {
            let Ok(entry) = entry else {
                continue;
            };
            if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path()) else {
                continue;
            };
            let Ok(item) = serde_json::from_slice::<RelayItem>(&bytes) else {
                continue;
            };
            if after.is_some_and(|cursor| item.id.as_str() <= cursor) {
                continue;
            }
            items.push(item);
        }

        items.sort_by(|a, b| a.id.cmp(&b.id));
        items.truncate(limit.clamp(1, 200));
        Ok(items)
    }

    pub fn count(&self, space: &str) -> io::Result<usize> {
        let dir = self.space_dir(space);
        if !dir.exists() {
            return Ok(0);
        }
        Ok(fs::read_dir(dir)?
            .flatten()
            .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
            .count())
    }

    fn space_dir(&self, space: &str) -> PathBuf {
        self.root
            .join("spaces")
            .join(sanitize_key(space, "default"))
    }
}

pub fn create_relay_router(store: RelayStore, default_space: String) -> Router {
    let state = RelayState {
        store,
        default_space: sanitize_key(&default_space, "default"),
    };

    Router::new()
        .route("/", get(relay_index))
        .route("/api/relay/status", get(relay_status))
        .route("/api/devices/hello", post(relay_device_hello))
        .route("/api/capture/text", post(relay_capture_text))
        .route("/api/capture/drop", post(relay_capture_drop))
        .route("/api/relay/push", post(relay_push))
        .route("/api/relay/pull", get(relay_pull))
        .with_state(state)
        .layer(DefaultBodyLimit::max(MAX_RELAY_UPLOAD_BYTES))
        .layer(CorsLayer::permissive())
}

async fn relay_index() -> Html<&'static str> {
    Html(
        r#"<!doctype html>
<meta charset="utf-8">
<title>AEON Relay</title>
<style>
body{font:16px system-ui;margin:40px;line-height:1.5;color:#f2f0e7;background:#10110f}
code{background:#20231b;padding:2px 6px;border-radius:4px}
</style>
<h1>AEON Relay</h1>
<p>This is an AEON built-in relay node. Point Android to this URL. Desktop AEON can connect with <code>scripts\aeon.ps1 -Mode desktop -RelayUrl &lt;this-url&gt;</code>, or run the full local stack with <code>scripts\aeon.ps1</code>.</p>
"#,
    )
}

async fn relay_status(State(state): State<RelayState>) -> Json<serde_json::Value> {
    let count = state.store.count(&state.default_space).unwrap_or(0);
    Json(serde_json::json!({
        "ok": true,
        "kind": "aeon-relay",
        "space": state.default_space,
        "items": count,
    }))
}

async fn relay_device_hello(
    State(state): State<RelayState>,
    Json(payload): Json<RelayDeviceHello>,
) -> Json<serde_json::Value> {
    let id = payload
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .unwrap_or("unknown-device");
    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown device");
    let kind = payload
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .unwrap_or("unknown");

    Json(serde_json::json!({
        "ok": true,
        "id": id,
        "name": name,
        "kind": kind,
        "relay": true,
        "space": state.default_space,
        "endpoint": payload.endpoint,
    }))
}

async fn relay_capture_text(
    State(state): State<RelayState>,
    headers: HeaderMap,
    Json(payload): Json<RelayTextPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let data = payload.text.into_bytes();
    if data.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let peer = peer_headers(&headers);
    let space = request_space(&headers, &state.default_space);
    let title = payload.title.or_else(|| {
        std::str::from_utf8(&data)
            .ok()?
            .lines()
            .next()
            .map(|s| s.chars().take(60).collect())
    });
    let item = make_relay_item(RelayItemInput {
        space,
        peer,
        data,
        kind: "Text".to_string(),
        title,
        source: payload.source.or_else(|| Some("Android".to_string())),
        filename: None,
        mime: "text/plain; charset=utf-8".to_string(),
    });

    let item = state
        .store
        .push(item)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "relay": true,
        "relay_id": item.id,
        "cid": item.cid,
    })))
}

async fn relay_capture_drop(
    State(state): State<RelayState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut captured = Vec::new();
    let peer = peer_headers(&headers);
    let space = request_space(&headers, &state.default_space);

    while let Ok(Some(field)) = multipart.next_field().await {
        let filename = field.file_name().unwrap_or("android-share").to_string();
        let content_type = field
            .content_type()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| {
                mime_guess::from_path(&filename)
                    .first_or_octet_stream()
                    .essence_str()
                    .to_string()
            });
        let data = field.bytes().await.map_err(|_| StatusCode::BAD_REQUEST)?;
        if data.is_empty() {
            continue;
        }

        let kind = aeon_capture::file::kind_from_path(Path::new(&filename), &data)
            .key()
            .to_string();
        let item = make_relay_item(RelayItemInput {
            space: space.clone(),
            peer: peer.clone(),
            data: data.to_vec(),
            kind,
            title: Some(filename.clone()),
            source: Some("Android".to_string()),
            filename: Some(filename.clone()),
            mime: content_type,
        });
        let item = state
            .store
            .push(item)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        captured.push(serde_json::json!({
            "name": filename,
            "relay_id": item.id,
            "cid": item.cid,
        }));
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "relay": true,
        "captured": captured,
    })))
}

async fn relay_push(
    State(state): State<RelayState>,
    Json(payload): Json<RelayPushPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut item = payload.item;
    if item.space.trim().is_empty() {
        item.space = state.default_space.clone();
    }
    let item = state
        .store
        .push(item)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "relay": true,
        "relay_id": item.id,
        "cid": item.cid,
    })))
}

async fn relay_pull(
    State(state): State<RelayState>,
    Query(query): Query<RelayPullQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let space = sanitize_key(
        query.space.as_deref().unwrap_or(&state.default_space),
        "default",
    );
    let items = state
        .store
        .pull(&space, query.after.as_deref(), query.limit.unwrap_or(50))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({
        "ok": true,
        "relay": true,
        "space": space,
        "items": items,
    })))
}

pub fn spawn_pull_loop(config: RelayPullConfig) {
    tokio::spawn(async move {
        let mut cursor = read_cursor(&config.cursor_path);
        let mut ticker = tokio::time::interval(Duration::from_secs(2));

        loop {
            ticker.tick().await;
            match pull_once(&config, cursor.as_deref()).await {
                Ok(Some(next_cursor)) => {
                    cursor = Some(next_cursor.clone());
                    if let Err(err) = write_cursor(&config.cursor_path, &next_cursor).await {
                        tracing::debug!("failed to save relay cursor: {err}");
                    }
                }
                Ok(None) => {}
                Err(err) => tracing::debug!("relay pull failed: {err}"),
            }
        }
    });
}

pub fn spawn_push_loop(config: RelayPushConfig) {
    let mut rx = config.capture_engine.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(entry) if should_push_to_relay(&entry) => {
                    match push_capture_entry(&config, &entry).await {
                        Ok(response) => tracing::debug!(
                            "relay pushed cid={} relay_id={}",
                            response.cid.as_deref().unwrap_or(""),
                            response.relay_id.as_deref().unwrap_or("")
                        ),
                        Err(err) => tracing::debug!("relay push failed: {err}"),
                    }
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::debug!("relay push lagged by {skipped} capture events");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

pub async fn push_capture_entry(
    config: &RelayPushConfig,
    entry: &CaptureEntry,
) -> Result<RelayPushResponse, Box<dyn std::error::Error + Send + Sync>> {
    let item = relay_item_from_entry(config, entry);
    let response = relay_push_http(&config.url, item).await?;
    if !response.ok {
        return Err(boxed_error("relay rejected push"));
    }
    Ok(response)
}

fn should_push_to_relay(entry: &CaptureEntry) -> bool {
    !entry.data.is_empty()
        && !matches!(entry.source, CaptureSource::PeerSync { .. })
        && !entry.meta.extra.contains_key("relay_id")
}

fn relay_item_from_entry(config: &RelayPushConfig, entry: &CaptureEntry) -> RelayItem {
    let peer = PeerHeaders {
        device_id: config.device_id.clone(),
        device_name: config.device_name.clone(),
        device_kind: config.device_kind.clone(),
    };
    let mut item = make_relay_item(RelayItemInput {
        space: sanitize_key(&config.space, "default"),
        peer,
        data: entry.data.clone(),
        kind: entry.kind.key().to_string(),
        title: entry.meta.title.clone(),
        source: relay_source(entry),
        filename: relay_filename(entry),
        mime: entry.mime(),
    });
    item.captured_at = entry.captured_at;
    item.cid = hex_cid(&entry.cid);
    item.id = relay_id(item.captured_at, &item.cid, &item.from_device_id);
    item
}

fn relay_source(entry: &CaptureEntry) -> Option<String> {
    entry.meta.app_name.clone().or_else(|| match &entry.source {
        CaptureSource::DragDrop => Some("DragDrop".to_string()),
        CaptureSource::Clipboard => Some("Clipboard".to_string()),
        CaptureSource::Screenshot => Some("Screenshot".to_string()),
        CaptureSource::FileWatch { path } => Some(format!("FileWatch: {path}")),
        CaptureSource::AppApi { app } => Some(app.clone()),
        CaptureSource::ShareMenu => Some("ShareMenu".to_string()),
        CaptureSource::Manual => Some("AEON".to_string()),
        CaptureSource::PeerSync { .. } => None,
    })
}

fn relay_filename(entry: &CaptureEntry) -> Option<String> {
    entry
        .meta
        .file_path
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
}

async fn pull_once(
    config: &RelayPullConfig,
    cursor: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let payload = relay_pull_http(&config.url, &config.space, cursor).await?;
    let mut latest = cursor.map(ToOwned::to_owned);

    for item in payload.items {
        let item_id = item.id.clone();
        if item.from_device_id != config.device_id {
            import_relay_item(config, item).await?;
        }
        latest = Some(item_id);
    }

    Ok(latest.filter(|next| cursor != Some(next.as_str())))
}

async fn relay_pull_http(
    base_url: &str,
    space: &str,
    cursor: Option<&str>,
) -> Result<RelayPullResponse, Box<dyn std::error::Error + Send + Sync>> {
    let endpoint = parse_http_url(base_url).map_err(boxed_error)?;
    let mut path = join_path(&endpoint.base_path, "/api/relay/pull");
    path.push_str("?space=");
    path.push_str(&url_encode(space));
    path.push_str("&limit=50");
    if let Some(cursor) = cursor.filter(|cursor| !cursor.is_empty()) {
        path.push_str("&after=");
        path.push_str(&url_encode(cursor));
    }

    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port)).await?;
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        host = endpoint.host_header()
    );
    stream.write_all(request.as_bytes()).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let (headers, body) = split_http_response(&response).map_err(boxed_error)?;
    if !headers.starts_with("HTTP/1.1 200") && !headers.starts_with("HTTP/1.0 200") {
        let status = headers.lines().next().unwrap_or("HTTP error");
        return Err(boxed_error(status.to_string()));
    }

    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        decode_chunked_body(body).map_err(boxed_error)?
    } else {
        body.to_vec()
    };

    Ok(serde_json::from_slice(&body)?)
}

async fn relay_push_http(
    base_url: &str,
    item: RelayItem,
) -> Result<RelayPushResponse, Box<dyn std::error::Error + Send + Sync>> {
    let endpoint = parse_http_url(base_url).map_err(boxed_error)?;
    let path = join_path(&endpoint.base_path, "/api/relay/push");
    let body = serde_json::to_vec(&RelayPushPayload { item })?;

    let mut stream = TcpStream::connect((endpoint.host.as_str(), endpoint.port)).await?;
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\nAccept: application/json\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n",
        host = endpoint.host_header(),
        len = body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(&body).await?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let (headers, body) = split_http_response(&response).map_err(boxed_error)?;
    if !headers.starts_with("HTTP/1.1 200") && !headers.starts_with("HTTP/1.0 200") {
        let status = headers.lines().next().unwrap_or("HTTP error");
        return Err(boxed_error(status.to_string()));
    }

    let body = if headers
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        decode_chunked_body(body).map_err(boxed_error)?
    } else {
        body.to_vec()
    };

    Ok(serde_json::from_slice(&body)?)
}

#[derive(Debug)]
struct HttpEndpoint {
    host: String,
    port: u16,
    base_path: String,
}

impl HttpEndpoint {
    fn host_header(&self) -> String {
        if self.port == 80 {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn parse_http_url(raw: &str) -> Result<HttpEndpoint, String> {
    let trimmed = raw.trim().trim_end_matches('/');
    let rest = trimmed.strip_prefix("http://").ok_or_else(|| {
        "AEON Relay pull currently uses built-in HTTP; use http://host:port".to_string()
    })?;
    let (host_port, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, port) = if let Some((host, port)) = host_port.rsplit_once(':') {
        (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|_| "invalid relay port".to_string())?,
        )
    } else {
        (host_port.to_string(), 80)
    };
    if host.trim().is_empty() {
        return Err("missing relay host".to_string());
    }
    Ok(HttpEndpoint {
        host,
        port,
        base_path: if path.is_empty() {
            String::new()
        } else {
            format!("/{path}")
        },
    })
}

fn join_path(base: &str, child: &str) -> String {
    if base.is_empty() {
        child.to_string()
    } else {
        format!(
            "{}/{}",
            base.trim_end_matches('/'),
            child.trim_start_matches('/')
        )
    }
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn split_http_response(response: &[u8]) -> Result<(&str, &[u8]), String> {
    let marker = b"\r\n\r\n";
    let Some(pos) = response
        .windows(marker.len())
        .position(|window| window == marker)
    else {
        return Err("invalid HTTP response".to_string());
    };
    let headers =
        std::str::from_utf8(&response[..pos]).map_err(|_| "invalid HTTP headers".to_string())?;
    Ok((headers, &response[pos + marker.len()..]))
}

fn decode_chunked_body(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::new();
    let mut pos = 0usize;
    loop {
        let Some(line_end) = find_crlf(&body[pos..]).map(|offset| pos + offset) else {
            return Err("invalid chunked body".to_string());
        };
        let size_line = std::str::from_utf8(&body[pos..line_end])
            .map_err(|_| "invalid chunk size".to_string())?;
        let size_text = size_line.split(';').next().unwrap_or("").trim();
        let size =
            usize::from_str_radix(size_text, 16).map_err(|_| "invalid chunk size".to_string())?;
        pos = line_end + 2;
        if size == 0 {
            break;
        }
        if pos + size > body.len() {
            return Err("truncated chunked body".to_string());
        }
        decoded.extend_from_slice(&body[pos..pos + size]);
        pos += size;
        if body.get(pos..pos + 2) != Some(b"\r\n") {
            return Err("invalid chunk terminator".to_string());
        }
        pos += 2;
    }
    Ok(decoded)
}

fn find_crlf(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|window| window == b"\r\n")
}

fn boxed_error(message: impl Into<String>) -> Box<dyn std::error::Error + Send + Sync> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

async fn import_relay_item(
    config: &RelayPullConfig,
    item: RelayItem,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let data = BASE64.decode(&item.data_base64)?;
    if data.is_empty() {
        return Ok(());
    }

    let kind = kind_from_relay_item(&item, &data);
    let mut entry = CaptureEntry::new(
        data,
        kind,
        CaptureSource::PeerSync {
            device_name: item.from_device_name.clone(),
        },
    );
    if let Some(title) = &item.title {
        entry.meta.title = Some(title.clone());
    }
    entry.meta.app_name = item
        .source
        .clone()
        .or_else(|| Some("AEON Relay".to_string()));
    entry.meta.file_path = item.filename.clone();
    entry
        .meta
        .extra
        .insert("relay_id".to_string(), item.id.clone());
    entry
        .meta
        .extra
        .insert("relay_space".to_string(), item.space.clone());
    entry.meta.extra.insert(
        "relay_from_device_id".to_string(),
        item.from_device_id.clone(),
    );
    entry
        .meta
        .extra
        .insert("relay_from_kind".to_string(), item.from_kind.clone());
    entry
        .meta
        .extra
        .insert("relay_mime".to_string(), item.mime.clone());

    config.capture_engine.capture(entry).await?;
    tracing::info!(
        "relay imported {} from {} into {}",
        item.id,
        item.from_device_name,
        config.device_name
    );
    Ok(())
}

fn kind_from_relay_item(item: &RelayItem, data: &[u8]) -> CaptureKind {
    if let Some(filename) = &item.filename {
        return aeon_capture::file::kind_from_path(Path::new(filename), data);
    }

    match item.kind.as_str() {
        "Conversation" => CaptureKind::Conversation,
        "ProcessState" => CaptureKind::ProcessState,
        "VmSnapshot" => CaptureKind::VmSnapshot,
        "Clipboard" => CaptureKind::Clipboard,
        "Webpage" => CaptureKind::Webpage,
        "Code" => CaptureKind::Code {
            language: item
                .mime
                .strip_prefix("text/x-")
                .unwrap_or("code")
                .to_string(),
        },
        "Document" => CaptureKind::Document {
            format: item
                .mime
                .strip_prefix("application/")
                .unwrap_or("document")
                .to_string(),
        },
        "Image" => {
            let format = item
                .mime
                .strip_prefix("image/")
                .unwrap_or("png")
                .split(';')
                .next()
                .unwrap_or("png")
                .to_string();
            let (width, height) =
                aeon_capture::screenshot::image_dimensions(data).unwrap_or((0, 0));
            CaptureKind::Image {
                width,
                height,
                format,
            }
        }
        "Text" => std::str::from_utf8(data)
            .ok()
            .map(aeon_capture::clipboard::detect_text_kind)
            .unwrap_or(CaptureKind::Text),
        _ if item.mime.starts_with("text/") => std::str::from_utf8(data)
            .ok()
            .map(aeon_capture::clipboard::detect_text_kind)
            .unwrap_or(CaptureKind::Text),
        _ => CaptureKind::Blob {
            mime: item.mime.clone(),
        },
    }
}

struct RelayItemInput {
    space: String,
    peer: PeerHeaders,
    data: Vec<u8>,
    kind: String,
    title: Option<String>,
    source: Option<String>,
    filename: Option<String>,
    mime: String,
}

fn make_relay_item(input: RelayItemInput) -> RelayItem {
    let captured_at = now_ms();
    let cid = blake3::hash(&input.data).to_hex().to_string();
    let id = relay_id(captured_at, &cid, &input.peer.device_id);
    RelayItem {
        id,
        space: input.space,
        from_device_id: input.peer.device_id,
        from_device_name: input.peer.device_name,
        from_kind: input.peer.device_kind,
        kind: input.kind,
        title: input.title,
        source: input.source,
        filename: input.filename,
        mime: input.mime,
        cid,
        data_base64: BASE64.encode(input.data),
        captured_at,
    }
}

fn peer_headers(headers: &HeaderMap) -> PeerHeaders {
    PeerHeaders {
        device_id: header_text(headers, "x-aeon-device-id")
            .unwrap_or_else(|| "unknown-device".to_string()),
        device_name: header_text(headers, "x-aeon-device-name")
            .unwrap_or_else(|| "Unknown device".to_string()),
        device_kind: header_text(headers, "x-aeon-device-kind")
            .unwrap_or_else(|| "unknown".to_string()),
    }
}

fn request_space(headers: &HeaderMap, default_space: &str) -> String {
    header_text(headers, "x-aeon-space")
        .map(|space| sanitize_key(&space, default_space))
        .unwrap_or_else(|| default_space.to_string())
}

fn header_text(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)?
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn relay_id(captured_at: u64, cid: &str, device_id: &str) -> String {
    let short_cid: String = cid.chars().take(16).collect();
    let short_device: String = sanitize_key(device_id, "device").chars().take(16).collect();
    format!("{captured_at:020}-{short_cid}-{short_device}")
}

fn sanitize_key(value: &str, fallback: &str) -> String {
    let mut out = String::new();
    for ch in value.trim().chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches('.').trim_matches('-').to_string();
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

fn read_cursor(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

async fn write_cursor(path: &Path, cursor: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, cursor).await
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

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-relay-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_item(space: &str, text: &str) -> RelayItem {
        make_relay_item(RelayItemInput {
            space: space.to_string(),
            peer: PeerHeaders {
                device_id: "android-test".to_string(),
                device_name: "Android Test".to_string(),
                device_kind: "android".to_string(),
            },
            data: text.as_bytes().to_vec(),
            kind: "Text".to_string(),
            title: Some("test".to_string()),
            source: Some("Android".to_string()),
            filename: None,
            mime: "text/plain".to_string(),
        })
    }

    #[test]
    fn store_pulls_items_after_cursor() {
        let dir = temp_dir("cursor");
        let store = RelayStore::new(dir.clone());
        let first = store.push(test_item("home", "one")).unwrap();
        let second = store.push(test_item("home", "two")).unwrap();

        let pulled = store.pull("home", Some(&first.id), 50).unwrap();
        assert_eq!(pulled.len(), 1);
        assert_eq!(pulled[0].id, second.id);

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn store_sanitizes_space_names() {
        let dir = temp_dir("space");
        let store = RelayStore::new(dir.clone());
        let item = store.push(test_item("../home", "safe")).unwrap();

        assert_eq!(item.space, "home");
        assert_eq!(store.pull("home", None, 50).unwrap().len(), 1);

        let _ = fs::remove_dir_all(dir);
    }
}
