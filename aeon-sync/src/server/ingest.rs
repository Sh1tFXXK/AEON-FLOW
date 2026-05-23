use super::*;

use super::shared::*;

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
