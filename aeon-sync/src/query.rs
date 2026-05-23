use crate::server::AppState;
use aeon_capture::{hex_cid, CaptureRecord};
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

const DEFAULT_LIMIT: usize = 20;
const MAX_LIMIT: usize = 100;
const DAY_MS: u64 = 86_400_000;
const PLANNER_TIMEOUT_SECONDS: u64 = 20;
const PLANNER_SYSTEM_PROMPT: &str = "You convert AEON capture questions into a JSON object. Return only JSON, no markdown. Allowed fields: text string, kind string, time_range object with numeric Unix millisecond from/to fields, and limit number. Omit unknown fields instead of guessing.";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct QueryRequest {
    pub question: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct StructuredQueryPlan {
    pub text: Option<String>,
    pub kind: Option<String>,
    pub time_range: Option<QueryTimeRange>,
    pub limit: Option<usize>,
}

impl StructuredQueryPlan {
    fn has_effective_filter(&self) -> bool {
        self.text
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty())
            || self
                .kind
                .as_deref()
                .is_some_and(|kind| !kind.trim().is_empty())
            || self
                .time_range
                .as_ref()
                .is_some_and(QueryTimeRange::has_bound)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct QueryTimeRange {
    pub from: Option<u64>,
    pub to: Option<u64>,
}

impl QueryTimeRange {
    fn has_bound(&self) -> bool {
        self.from.is_some() || self.to.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryPlanError {
    InvalidJson,
    ExpectedObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPlannerProvider {
    OpenAiCompatible,
    OllamaChat,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlannerConfig {
    pub url: String,
    pub model: String,
    pub provider: QueryPlannerProvider,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryPlannerConfigError {
    InvalidProvider(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerHttpRequest {
    pub url: String,
    pub authorization: Option<String>,
    pub body: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryPlannerError {
    Http(String),
    InvalidResponse,
    InvalidPlan(QueryPlanError),
}

impl QueryPlannerProvider {
    fn parse(value: &str) -> Result<Self, QueryPlannerConfigError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai" | "openai-compatible" | "chat-completions" => Ok(Self::OpenAiCompatible),
            "ollama" | "ollama-chat" => Ok(Self::OllamaChat),
            other => Err(QueryPlannerConfigError::InvalidProvider(other.to_string())),
        }
    }
}

impl QueryPlannerConfig {
    pub fn new(
        url: impl Into<String>,
        model: impl Into<String>,
        provider: QueryPlannerProvider,
    ) -> Self {
        Self {
            url: url.into(),
            model: model.into(),
            provider,
            api_key: None,
        }
    }

    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    pub fn from_env() -> Result<Option<Self>, QueryPlannerConfigError> {
        let Some(url) = env_value("AEON_QUERY_PLANNER_URL") else {
            return Ok(None);
        };
        let Some(model) = env_value("AEON_QUERY_PLANNER_MODEL") else {
            return Ok(None);
        };
        let provider = match env_value("AEON_QUERY_PLANNER_PROVIDER") {
            Some(value) => QueryPlannerProvider::parse(&value)?,
            None => QueryPlannerProvider::OpenAiCompatible,
        };
        Ok(Some(Self {
            url,
            model,
            provider,
            api_key: env_value("AEON_QUERY_PLANNER_API_KEY"),
        }))
    }
}

pub async fn query(
    State(state): State<AppState>,
    Json(request): Json<QueryRequest>,
) -> Json<QueryResponse> {
    let now = now_ms();
    let plan = match (
        state.query_planner.as_ref(),
        request
            .question
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(config), Some(question)) => {
            match request_structured_plan(config, question, now).await {
                Ok(plan) => Some(plan),
                Err(err) => {
                    tracing::warn!(
                        ?err,
                        "AEON query planner failed; using deterministic fallback"
                    );
                    None
                }
            }
        }
        _ => None,
    };
    Json(run_query_with_optional_plan_at(
        request,
        plan,
        state.capture_engine.list().await,
        now,
    ))
}

pub async fn query_structured(
    State(state): State<AppState>,
    Json(plan): Json<StructuredQueryPlan>,
) -> Json<QueryResponse> {
    Json(run_structured_query_at(
        plan,
        state.capture_engine.list().await,
        now_ms(),
    ))
}

pub fn parse_llm_plan_json(input: &str) -> Result<StructuredQueryPlan, QueryPlanError> {
    let json = extract_first_json_object(input).ok_or(QueryPlanError::InvalidJson)?;
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|_| QueryPlanError::InvalidJson)?;
    if !value.is_object() {
        return Err(QueryPlanError::ExpectedObject);
    }
    serde_json::from_value(value).map_err(|_| QueryPlanError::InvalidJson)
}

pub fn build_planner_http_request(
    config: &QueryPlannerConfig,
    question: &str,
    now_ms: u64,
) -> PlannerHttpRequest {
    let user_content = format!("current_unix_ms={now_ms}\nquestion={question}");
    let messages = json!([
        {
            "role": "system",
            "content": PLANNER_SYSTEM_PROMPT,
        },
        {
            "role": "user",
            "content": user_content,
        }
    ]);
    let body = match config.provider {
        QueryPlannerProvider::OpenAiCompatible => json!({
            "model": config.model,
            "temperature": 0,
            "response_format": { "type": "json_object" },
            "messages": messages,
        }),
        QueryPlannerProvider::OllamaChat => json!({
            "model": config.model,
            "stream": false,
            "messages": messages,
        }),
    };

    PlannerHttpRequest {
        url: config.url.clone(),
        authorization: config
            .api_key
            .as_ref()
            .map(|api_key| format!("Bearer {api_key}")),
        body,
    }
}

pub fn parse_planner_response(
    provider: QueryPlannerProvider,
    response: &str,
) -> Result<StructuredQueryPlan, QueryPlannerError> {
    let value: serde_json::Value =
        serde_json::from_str(response).map_err(|_| QueryPlannerError::InvalidResponse)?;
    let content = match provider {
        QueryPlannerProvider::OpenAiCompatible => value
            .pointer("/choices/0/message/content")
            .or_else(|| value.pointer("/choices/0/text"))
            .and_then(serde_json::Value::as_str),
        QueryPlannerProvider::OllamaChat => value
            .pointer("/message/content")
            .or_else(|| value.get("response"))
            .and_then(serde_json::Value::as_str),
    }
    .ok_or(QueryPlannerError::InvalidResponse)?;

    parse_llm_plan_json(content).map_err(QueryPlannerError::InvalidPlan)
}

pub async fn request_structured_plan(
    config: &QueryPlannerConfig,
    question: &str,
    now_ms: u64,
) -> Result<StructuredQueryPlan, QueryPlannerError> {
    let planner_request = build_planner_http_request(config, question, now_ms);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(PLANNER_TIMEOUT_SECONDS))
        .build()
        .map_err(|err| QueryPlannerError::Http(err.to_string()))?;
    let mut request = client
        .post(&planner_request.url)
        .json(&planner_request.body);
    if let Some(authorization) = planner_request.authorization {
        request = request.header(reqwest::header::AUTHORIZATION, authorization);
    }
    let response = request
        .send()
        .await
        .map_err(|err| QueryPlannerError::Http(err.to_string()))?
        .error_for_status()
        .map_err(|err| QueryPlannerError::Http(err.to_string()))?
        .text()
        .await
        .map_err(|err| QueryPlannerError::Http(err.to_string()))?;

    parse_planner_response(config.provider, &response)
}

fn extract_first_json_object(input: &str) -> Option<&str> {
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, ch) in input.char_indices() {
        if start.is_none() {
            if ch == '{' {
                start = Some(index);
                depth = 1;
            }
            continue;
        }

        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return input.get(start?..=index);
                }
            }
            _ => {}
        }
    }

    None
}

pub fn run_query(request: QueryRequest, records: Vec<CaptureRecord>) -> QueryResponse {
    run_query_at(request, records, now_ms())
}

pub fn run_query_at(
    request: QueryRequest,
    records: Vec<CaptureRecord>,
    now_ms: u64,
) -> QueryResponse {
    if is_today_activity_question(request.question.as_deref()) {
        return run_today_summary(request, records, now_ms);
    }

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

pub fn run_query_with_optional_plan_at(
    request: QueryRequest,
    plan: Option<StructuredQueryPlan>,
    records: Vec<CaptureRecord>,
    now_ms: u64,
) -> QueryResponse {
    let Some(mut plan) = plan else {
        return run_query_at(request, records, now_ms);
    };
    if !plan.has_effective_filter() {
        return run_query_at(request, records, now_ms);
    }

    if plan.text.is_none() {
        plan.text = request.text;
    }
    if plan.kind.is_none() {
        plan.kind = request.kind;
    }
    if plan.limit.is_none() {
        plan.limit = request.limit;
    }

    run_structured_query_at(plan, records, now_ms)
}

pub fn run_structured_query_at(
    plan: StructuredQueryPlan,
    records: Vec<CaptureRecord>,
    now_ms: u64,
) -> QueryResponse {
    let request = QueryRequest {
        question: None,
        text: plan.text,
        kind: plan.kind,
        limit: plan.limit,
    };
    let mut response = run_query_at(request, records, now_ms);
    if let Some(range) = plan.time_range {
        let from = range.from.unwrap_or(0);
        let to = range.to.unwrap_or(now_ms);
        response
            .captures
            .retain(|capture| capture.captured_at >= from && capture.captured_at <= to);
        let count = response.captures.len();
        response.answer = if count == 1 {
            "Found 1 capture.".to_string()
        } else {
            format!("Found {count} captures.")
        };
    }
    response
}

fn run_today_summary(
    request: QueryRequest,
    records: Vec<CaptureRecord>,
    now_ms: u64,
) -> QueryResponse {
    let day_start = now_ms - (now_ms % DAY_MS);
    let limit = request.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    let mut captures = records
        .into_iter()
        .filter(|record| record.captured_at >= day_start && record.captured_at <= now_ms)
        .collect::<Vec<_>>();

    captures.sort_by(|a, b| b.captured_at.cmp(&a.captured_at).then(a.cid.cmp(&b.cid)));

    let mut kind_counts = std::collections::BTreeMap::<String, usize>::new();
    for record in &captures {
        *kind_counts
            .entry(record.kind.key().to_string())
            .or_default() += 1;
    }

    let highlights = captures
        .iter()
        .take(3)
        .map(|record| {
            record
                .meta
                .title
                .clone()
                .unwrap_or_else(|| record.kind.key().to_string())
        })
        .collect::<Vec<_>>();

    let count = captures.len();
    let kind_text = kind_counts
        .into_iter()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect::<Vec<_>>()
        .join(", ");
    let highlight_text = if highlights.is_empty() {
        "暂无活动".to_string()
    } else {
        highlights.join("；")
    };

    QueryResponse {
        answer: format!("今天记录了 {count} 条活动。类型：{kind_text}。最近：{highlight_text}。"),
        captures: captures
            .into_iter()
            .take(limit)
            .map(query_capture_result)
            .collect(),
    }
}

fn is_today_activity_question(question: Option<&str>) -> bool {
    let Some(question) = question.map(str::trim).filter(|value| !value.is_empty()) else {
        return false;
    };
    let normalized = question.to_ascii_lowercase();
    (question.contains("今天") || normalized.contains("today"))
        && (question.contains("做了什么")
            || question.contains("做过什么")
            || question.contains("活动")
            || normalized.contains("what did i do")
            || normalized.contains("what have i done"))
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn env_value(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
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
                question: None,
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
                question: None,
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
                question: None,
                text: None,
                kind: None,
                limit: Some(1),
            },
            records,
        );

        assert_eq!(response.captures.len(), 1);
        assert_eq!(response.answer, "Found 1 capture.");
    }

    #[test]
    fn today_question_summarizes_only_todays_activity() {
        const DAY_MS: u64 = 86_400_000;
        let now = DAY_MS * 3 + 12_000;
        let mut today = record("Edited AEON query", "query work", CaptureKind::Text);
        today.captured_at = DAY_MS * 3 + 1_000;
        let mut os_event = record(
            "Window focus: VS Code",
            "Code focused",
            CaptureKind::OsActivity,
        );
        os_event.captured_at = DAY_MS * 3 + 2_000;
        let mut yesterday = record("Old capture", "previous day", CaptureKind::Text);
        yesterday.captured_at = DAY_MS * 2 + 12_000;

        let response = run_query_at(
            QueryRequest {
                question: Some("今天我做了什么".to_string()),
                text: None,
                kind: None,
                limit: Some(10),
            },
            vec![today, os_event, yesterday],
            now,
        );

        assert_eq!(response.captures.len(), 2);
        assert!(response.answer.contains("今天记录了 2 条活动"));
        assert!(response.answer.contains("Text 1"));
        assert!(response.answer.contains("OsActivity 1"));
        assert!(response.answer.contains("Window focus: VS Code"));
        assert!(!response.answer.contains("Old capture"));
    }

    #[test]
    fn structured_llm_plan_filters_by_time_kind_and_text() {
        let plan = parse_llm_plan_json(
            r#"{
                "text": "context",
                "kind": "Text",
                "time_range": { "from": 1000, "to": 3000 },
                "limit": 5
            }"#,
        )
        .unwrap();
        let mut matching = record("AEON", "context bus notes", CaptureKind::Text);
        matching.captured_at = 2_000;
        let mut wrong_time = record("Old AEON", "context bus notes", CaptureKind::Text);
        wrong_time.captured_at = 500;
        let mut wrong_kind = record(
            "Image",
            "context diagram",
            CaptureKind::Image {
                width: 1,
                height: 1,
                format: "png".to_string(),
            },
        );
        wrong_kind.captured_at = 2_000;

        let response = run_structured_query_at(plan, vec![wrong_time, wrong_kind, matching], 4_000);

        assert_eq!(response.answer, "Found 1 capture.");
        assert_eq!(response.captures.len(), 1);
        assert_eq!(response.captures[0].title, "AEON");
    }

    #[test]
    fn structured_llm_plan_rejects_invalid_json_shape() {
        assert!(parse_llm_plan_json(r#"{"time_range":{"from":"today"}}"#).is_err());
        assert!(parse_llm_plan_json(r#"[]"#).is_err());
    }

    #[test]
    fn structured_llm_plan_extracts_fenced_json_response() {
        let plan = parse_llm_plan_json(
            r#"Use this plan:

```json
{"text":"AEON","kind":"Text","limit":3}
```
"#,
        )
        .unwrap();

        assert_eq!(plan.text.as_deref(), Some("AEON"));
        assert_eq!(plan.kind.as_deref(), Some("Text"));
        assert_eq!(plan.limit, Some(3));
    }

    #[test]
    fn openai_compatible_planner_request_uses_chat_completion_shape() {
        let config = QueryPlannerConfig::new(
            "https://planner.test/v1/chat/completions",
            "qwen2.5",
            QueryPlannerProvider::OpenAiCompatible,
        )
        .with_api_key("secret");

        let request = build_planner_http_request(&config, "today activity", 86_400_123);

        assert_eq!(request.url, "https://planner.test/v1/chat/completions");
        assert_eq!(request.authorization.as_deref(), Some("Bearer secret"));
        assert_eq!(request.body["model"], "qwen2.5");
        assert_eq!(request.body["temperature"], 0);
        assert_eq!(request.body["messages"][0]["role"], "system");
        assert_eq!(request.body["messages"][1]["role"], "user");
        assert!(request.body["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("current_unix_ms=86400123"));
    }

    #[test]
    fn ollama_planner_request_uses_chat_shape_without_authorization() {
        let config = QueryPlannerConfig::new(
            "http://127.0.0.1:11434/api/chat",
            "qwen2.5:7b",
            QueryPlannerProvider::OllamaChat,
        );

        let request = build_planner_http_request(&config, "find AEON", 123);

        assert_eq!(request.url, "http://127.0.0.1:11434/api/chat");
        assert_eq!(request.authorization, None);
        assert_eq!(request.body["model"], "qwen2.5:7b");
        assert_eq!(request.body["stream"], false);
        assert_eq!(request.body["messages"][0]["role"], "system");
        assert_eq!(request.body["messages"][1]["role"], "user");
    }

    #[test]
    fn openai_compatible_planner_response_extracts_structured_plan() {
        let plan = parse_planner_response(
            QueryPlannerProvider::OpenAiCompatible,
            r#"{
                "choices": [{
                    "message": {
                        "content": "```json\n{\"text\":\"AEON\",\"kind\":\"Text\",\"limit\":3}\n```"
                    }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(plan.text.as_deref(), Some("AEON"));
        assert_eq!(plan.kind.as_deref(), Some("Text"));
        assert_eq!(plan.limit, Some(3));
    }

    #[test]
    fn ollama_planner_response_extracts_structured_plan() {
        let plan = parse_planner_response(
            QueryPlannerProvider::OllamaChat,
            r#"{
                "message": {
                    "content": "{\"text\":\"browser\",\"kind\":\"Webpage\",\"limit\":5}"
                }
            }"#,
        )
        .unwrap();

        assert_eq!(plan.text.as_deref(), Some("browser"));
        assert_eq!(plan.kind.as_deref(), Some("Webpage"));
        assert_eq!(plan.limit, Some(5));
    }

    #[test]
    fn query_uses_structured_plan_when_planner_succeeds() {
        let records = vec![
            record("AEON code", "context query work", CaptureKind::Text),
            record("Lunch", "outside scope", CaptureKind::Text),
        ];
        let response = run_query_with_optional_plan_at(
            QueryRequest {
                question: Some("find AEON context".to_string()),
                text: None,
                kind: None,
                limit: Some(10),
            },
            Some(StructuredQueryPlan {
                text: Some("context".to_string()),
                kind: Some("Text".to_string()),
                time_range: None,
                limit: None,
            }),
            records,
            500,
        );

        assert_eq!(response.answer, "Found 1 capture.");
        assert_eq!(response.captures[0].title, "AEON code");
    }

    #[test]
    fn query_falls_back_when_planner_is_unavailable() {
        let records = vec![record(
            "AEON design",
            "context bus notes",
            CaptureKind::Text,
        )];

        let response = run_query_with_optional_plan_at(
            QueryRequest {
                question: Some("find context".to_string()),
                text: Some("context".to_string()),
                kind: None,
                limit: Some(10),
            },
            None,
            records,
            500,
        );

        assert_eq!(response.answer, "Found 1 capture.");
        assert_eq!(response.captures[0].title, "AEON design");
    }

    #[test]
    fn query_falls_back_when_planner_returns_empty_plan() {
        let now = DAY_MS * 3 + 12_000;
        let mut today = record("Edited AEON query", "query work", CaptureKind::Text);
        today.captured_at = DAY_MS * 3 + 1_000;
        let mut yesterday = record("Old capture", "previous day", CaptureKind::Text);
        yesterday.captured_at = DAY_MS * 2 + 12_000;

        let response = run_query_with_optional_plan_at(
            QueryRequest {
                question: Some("today what did i do".to_string()),
                text: None,
                kind: None,
                limit: Some(10),
            },
            Some(StructuredQueryPlan {
                text: None,
                kind: None,
                time_range: None,
                limit: None,
            }),
            vec![today, yesterday],
            now,
        );

        assert_eq!(response.captures.len(), 1);
        assert_eq!(response.captures[0].title, "Edited AEON query");
    }
}
