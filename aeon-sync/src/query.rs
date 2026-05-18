use crate::server::AppState;
use aeon_capture::{hex_cid, CaptureRecord};
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct QueryRequest {
    pub text: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueryResponse {
    pub answer: String,
    pub captures: Vec<QueryCaptureResult>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueryCaptureResult {
    pub cid: String,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub captured_at: u64,
}

pub async fn query(
    State(state): State<AppState>,
    Json(request): Json<QueryRequest>,
) -> Json<QueryResponse> {
    Json(run_query(request, state.capture_engine.list().await))
}

pub fn run_query(request: QueryRequest, records: Vec<CaptureRecord>) -> QueryResponse {
    let text = request
        .text
        .as_ref()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());
    let kind = request
        .kind
        .as_ref()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    let captures = records
        .into_iter()
        .filter(|record| kind.as_ref().is_none_or(|kind| record.kind.key() == kind))
        .filter(|record| {
            text.as_ref()
                .is_none_or(|text| record_matches_text(record, text))
        })
        .take(limit)
        .map(query_capture_result)
        .collect::<Vec<_>>();

    let count = captures.len();
    QueryResponse {
        answer: if count == 1 {
            "Found 1 capture.".to_string()
        } else {
            format!("Found {count} captures.")
        },
        captures,
    }
}

fn query_capture_result(record: CaptureRecord) -> QueryCaptureResult {
    let title = record
        .meta
        .title
        .clone()
        .unwrap_or_else(|| record.kind.key().to_string());
    QueryCaptureResult {
        cid: hex_cid(&record.cid),
        kind: record.kind.key().to_string(),
        title,
        summary: record.meta.summary,
        captured_at: record.captured_at,
    }
}

fn record_matches_text(record: &CaptureRecord, text: &str) -> bool {
    let mut haystack = String::new();
    append_search_field(&mut haystack, record.meta.title.as_deref());
    append_search_field(&mut haystack, record.meta.summary.as_deref());
    append_search_field(&mut haystack, record.meta.app_name.as_deref());
    append_search_field(&mut haystack, record.meta.file_path.as_deref());
    append_search_field(&mut haystack, record.meta.url.as_deref());
    for value in record.meta.extra.values() {
        append_search_field(&mut haystack, Some(value));
    }
    haystack.to_ascii_lowercase().contains(text)
}

fn append_search_field(haystack: &mut String, value: Option<&str>) {
    if let Some(value) = value {
        haystack.push(' ');
        haystack.push_str(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeon_capture::{CaptureKind, CaptureMetadata, CaptureRecord, CaptureSource};

    fn record(title: &str, summary: &str, kind: CaptureKind) -> CaptureRecord {
        let meta = CaptureMetadata {
            title: Some(title.to_string()),
            summary: Some(summary.to_string()),
            ..Default::default()
        };
        CaptureRecord {
            cid: [1u8; 32],
            kind,
            meta,
            source: CaptureSource::Manual,
            captured_at: 100,
            by: [0u8; 32],
            device: [0u8; 16],
            size: summary.len(),
            mime: "text/plain".to_string(),
        }
    }

    #[test]
    fn query_filters_captures_by_text() {
        let records = vec![
            record("AEON design", "context bus notes", CaptureKind::Text),
            record("Lunch", "no project content", CaptureKind::Text),
        ];

        let response = run_query(
            QueryRequest {
                text: Some("context".to_string()),
                kind: None,
                limit: Some(10),
            },
            records,
        );

        assert_eq!(response.captures.len(), 1);
        assert_eq!(response.captures[0].title, "AEON design");
    }

    #[test]
    fn query_filters_captures_by_kind() {
        let records = vec![
            record(
                "Image",
                "photo",
                CaptureKind::Image {
                    width: 1,
                    height: 1,
                    format: "png".to_string(),
                },
            ),
            record("Text", "note", CaptureKind::Text),
        ];

        let response = run_query(
            QueryRequest {
                text: None,
                kind: Some("Text".to_string()),
                limit: Some(10),
            },
            records,
        );

        assert_eq!(response.captures.len(), 1);
        assert_eq!(response.captures[0].title, "Text");
    }

    #[test]
    fn query_returns_bounded_stable_summary() {
        let records = vec![
            record("One", "first", CaptureKind::Text),
            record("Two", "second", CaptureKind::Text),
        ];

        let response = run_query(
            QueryRequest {
                text: None,
                kind: None,
                limit: Some(1),
            },
            records,
        );

        assert_eq!(response.captures.len(), 1);
        assert_eq!(response.answer, "Found 1 capture.");
    }
}
