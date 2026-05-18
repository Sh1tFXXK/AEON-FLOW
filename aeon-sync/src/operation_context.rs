use crate::server::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct OperationContext {
    pub current_task: Option<TaskContext>,
    pub clipboard: Option<ClipboardContext>,
    pub scratch_pad: String,
    pub ai_sessions: Vec<AiSessionContext>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TaskContext {
    pub id: String,
    pub title: String,
    pub started_at: u64,
    pub source: ContextSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardContext {
    pub text: String,
    pub captured_at: u64,
    pub source_device: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiSessionContext {
    pub id: String,
    pub provider: String,
    pub account: String,
    pub conversation_cid: Option<String>,
    pub last_updated: u64,
    pub device: String,
    pub resumable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ContextSource {
    Desktop,
    Android,
    BrowserExtension,
    Custom { name: String },
}

#[derive(Debug)]
pub struct ContextStore {
    path: PathBuf,
    context: OperationContext,
}

#[derive(Debug, Deserialize)]
pub struct TaskUpdatePayload {
    pub task: Option<TaskContext>,
}

#[derive(Debug, Deserialize)]
pub struct ClipboardUpdatePayload {
    pub clipboard: Option<ClipboardContext>,
}

#[derive(Debug, Deserialize)]
pub struct ScratchUpdatePayload {
    pub text: String,
}

#[derive(Debug, Deserialize)]
pub struct AiSessionUpdatePayload {
    pub session: AiSessionContext,
}

impl ContextStore {
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let context = read_context(&path)?;
        Ok(Self { path, context })
    }

    pub fn snapshot(&self) -> OperationContext {
        self.context.clone()
    }

    pub fn set_task(&mut self, task: Option<TaskContext>) -> io::Result<()> {
        self.context.current_task = task;
        self.touch_and_persist()
    }

    pub fn set_clipboard(&mut self, clipboard: Option<ClipboardContext>) -> io::Result<()> {
        self.context.clipboard = clipboard;
        self.touch_and_persist()
    }

    pub fn set_scratch(&mut self, text: String) -> io::Result<()> {
        self.context.scratch_pad = text;
        self.touch_and_persist()
    }

    pub fn upsert_ai_session(&mut self, session: AiSessionContext) -> io::Result<()> {
        if let Some(existing) = self
            .context
            .ai_sessions
            .iter_mut()
            .find(|existing| existing.id == session.id)
        {
            *existing = session;
        } else {
            self.context.ai_sessions.push(session);
        }
        self.context
            .ai_sessions
            .sort_by(|a, b| b.last_updated.cmp(&a.last_updated).then(a.id.cmp(&b.id)));
        self.touch_and_persist()
    }

    fn touch_and_persist(&mut self) -> io::Result<()> {
        self.context.updated_at = now_ms();
        write_context(&self.path, &self.context)
    }
}

pub async fn get_context(State(state): State<AppState>) -> Json<OperationContext> {
    Json(state.operation_context.lock().await.snapshot())
}

pub async fn set_task(
    State(state): State<AppState>,
    Json(payload): Json<TaskUpdatePayload>,
) -> Result<Json<OperationContext>, StatusCode> {
    update_context(state, |store| store.set_task(payload.task)).await
}

pub async fn set_clipboard(
    State(state): State<AppState>,
    Json(payload): Json<ClipboardUpdatePayload>,
) -> Result<Json<OperationContext>, StatusCode> {
    update_context(state, |store| store.set_clipboard(payload.clipboard)).await
}

pub async fn set_scratch(
    State(state): State<AppState>,
    Json(payload): Json<ScratchUpdatePayload>,
) -> Result<Json<OperationContext>, StatusCode> {
    update_context(state, |store| store.set_scratch(payload.text)).await
}

pub async fn upsert_ai_session(
    State(state): State<AppState>,
    Json(payload): Json<AiSessionUpdatePayload>,
) -> Result<Json<OperationContext>, StatusCode> {
    update_context(state, |store| store.upsert_ai_session(payload.session)).await
}

async fn update_context(
    state: AppState,
    update: impl FnOnce(&mut ContextStore) -> io::Result<()>,
) -> Result<Json<OperationContext>, StatusCode> {
    let mut store = state.operation_context.lock().await;
    update(&mut store).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(store.snapshot()))
}

fn read_context(path: &Path) -> io::Result<OperationContext> {
    if !path.exists() {
        return Ok(OperationContext::default());
    }
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

fn write_context(path: &Path, context: &OperationContext) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(context)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    std::fs::write(path, bytes)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-operation-context-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn default_context_has_no_active_state() {
        let context = OperationContext::default();

        assert!(context.current_task.is_none());
        assert!(context.clipboard.is_none());
        assert!(context.scratch_pad.is_empty());
        assert!(context.ai_sessions.is_empty());
    }

    #[test]
    fn store_updates_scratch_without_replacing_other_fields() {
        let dir = temp_dir();
        let mut store = ContextStore::new(dir.join("context.json")).unwrap();
        let task = TaskContext {
            id: "task-1".to_string(),
            title: "Implement context bus".to_string(),
            started_at: 1_771_000_001_000,
            source: ContextSource::Desktop,
        };

        store.set_task(Some(task.clone())).unwrap();
        store.set_scratch("temporary note".to_string()).unwrap();

        let snapshot = store.snapshot();
        assert_eq!(snapshot.current_task, Some(task));
        assert_eq!(snapshot.scratch_pad, "temporary note");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn ai_sessions_are_upserted_by_id() {
        let dir = temp_dir();
        let mut store = ContextStore::new(dir.join("context.json")).unwrap();
        let original = AiSessionContext {
            id: "claude-1".to_string(),
            provider: "claude".to_string(),
            account: "work".to_string(),
            conversation_cid: Some("abc".to_string()),
            last_updated: 10,
            device: "desktop".to_string(),
            resumable: true,
        };
        let mut updated = original.clone();
        updated.last_updated = 20;
        updated.device = "phone".to_string();

        store.upsert_ai_session(original).unwrap();
        store.upsert_ai_session(updated.clone()).unwrap();

        assert_eq!(store.snapshot().ai_sessions, vec![updated]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn store_round_trips_json_file() {
        let dir = temp_dir();
        let path = dir.join("context.json");
        let mut store = ContextStore::new(&path).unwrap();
        store
            .set_clipboard(Some(ClipboardContext {
                text: "shared text".to_string(),
                captured_at: 1_771_000_002_000,
                source_device: "desktop".to_string(),
            }))
            .unwrap();

        let restored = ContextStore::new(&path).unwrap().snapshot();

        assert_eq!(
            restored.clipboard.map(|clipboard| clipboard.text),
            Some("shared text".to_string())
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
