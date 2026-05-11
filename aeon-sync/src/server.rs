use axum::{
    body::Body,
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{delete, get, post},
    Router,
};
use serde::Serialize;
use std::path::{Component, PathBuf};
use tokio_util::io::ReaderStream;

#[derive(Clone)]
pub struct AppState {
    pub sync_dir: PathBuf,
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

                entries.push(FileEntry {
                    name,
                    size,
                    size_human: human_size(size),
                    mime,
                    modified,
                    cid,
                    is_dir,
                });
            }
        }
    }

    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Json(entries)
}

pub async fn download_file(Path(filename): Path<String>, State(state): State<AppState>) -> Response {
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
        uploaded.push(serde_json::json!({
            "name": safe_name,
            "size": data.len(),
            "cid": cid
        }));

        tracing::info!("Uploaded: {} ({} bytes)", filename, data.len());
    }

    Ok(Json(serde_json::json!({ "uploaded": uploaded })))
}

pub async fn delete_file(Path(filename): Path<String>, State(state): State<AppState>) -> StatusCode {
    let Some(safe_name) = sanitize_filename(&filename) else {
        return StatusCode::BAD_REQUEST;
    };

    let path = state.sync_dir.join(safe_name);
    if !path.starts_with(&state.sync_dir) {
        return StatusCode::FORBIDDEN;
    }

    match tokio::fs::remove_file(&path).await {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::NOT_FOUND,
    }
}

pub async fn index_page() -> Html<&'static str> {
    Html(include_str!("../static/index.html"))
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_page))
        .route("/api/files", get(list_files))
        .route("/api/upload", post(upload_file))
        .route("/api/download/:filename", get(download_file))
        .route("/api/files/:filename", delete(delete_file))
        .with_state(state)
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
    if path.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return None;
    }
    path.file_name()
        .and_then(|s| {
            let name = s.to_string_lossy().trim().to_string();
            if name.is_empty() { None } else { Some(name) }
        })
}
