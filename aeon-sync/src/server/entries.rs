use super::*;

use super::shared::*;

#[derive(Deserialize)]
pub struct EditEntryPayload {
    pub text: String,
    pub title: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct EntryListParams {
    pub kind: Option<String>,
    pub source: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Deserialize, Default)]
pub struct EventListParams {
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub limit: Option<usize>,
    pub kind: Option<String>,
    pub source: Option<String>,
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

pub async fn list_entries(
    Query(params): Query<EntryListParams>,
    State(state): State<AppState>,
) -> Json<Vec<CapturePayload>> {
    let limit = params.limit.unwrap_or(500).min(500);
    let mut entries: Vec<CapturePayload> = state
        .capture_engine
        .list()
        .await
        .into_iter()
        .map(capture_payload)
        .collect();

    if let Some(kind) = params.kind.as_deref() {
        let kind = kind.to_ascii_lowercase();
        entries.retain(|e| entry_matches_kind(&e.kind, &kind));
    }
    if let Some(source) = params.source.as_deref() {
        let source = source.to_ascii_lowercase();
        entries.retain(|e| e.source.to_ascii_lowercase() == source);
    }
    entries.truncate(limit);
    Json(entries)
}

fn entry_matches_kind(kind: &str, filter: &str) -> bool {
    kind.eq_ignore_ascii_case(filter)
        || kind.to_ascii_lowercase().starts_with(&filter.to_ascii_lowercase())
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
    let kind_filter = params.kind.clone();
    let source_filter = params.source.clone();
    let query = params.try_into_query()?;
    let mut events = state
        .event_log
        .lock()
        .await
        .list(query)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(kind) = kind_filter.as_deref() {
        let kind = kind.to_ascii_lowercase();
        events.retain(|event| event_matches_kind_filter(event, &kind));
    }
    if let Some(source) = source_filter.as_deref() {
        let source = source.to_ascii_lowercase();
        events.retain(|event| event_matches_source_filter(event, &source));
    }
    Ok(Json(events.into_iter().map(event_payload).collect()))
}

fn event_matches_kind_filter(event: &aeon_capture::AeonEvent, filter: &str) -> bool {
    let aeon_capture::EventKind::CaptureAdded(capture) = &event.kind;
    let key = capture.capture_kind.key();
    key.eq_ignore_ascii_case(filter) || key.to_ascii_lowercase().starts_with(filter)
}

fn event_matches_source_filter(event: &aeon_capture::AeonEvent, filter: &str) -> bool {
    match &event.source {
        aeon_capture::EventSource::LocalCapture(source) => {
            super::shared::source_key(source).eq_ignore_ascii_case(filter)
        }
        aeon_capture::EventSource::RelayImport { .. } => filter == "peersync",
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
