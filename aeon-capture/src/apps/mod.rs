use crate::capture::{CaptureEntry, CID};
use crate::engine::CaptureEngine;
use std::sync::Arc;

mod browser;
mod claude;
mod process;
mod util;
mod vm;
mod vscode;

pub use browser::BrowserCapture;
pub use claude::ClaudeDesktopCapture;
pub use process::{list_processes, ProcessStateCapture, RunningProcess};
pub use vm::{
    auto_wrap_capture_entry, capture_vm_snapshot, capture_vm_snapshot_from_state_dir, list_vms,
    list_vms_in_state_dir, set_vm_status, set_vm_status_in_state_dir,
    wrap_capture_entry_as_vm_snapshot, wrap_capture_entry_as_vm_snapshot_in_state_dir,
    AeonVmCapture, AeonVmInfo,
};
pub use vscode::VSCodeCapture;

pub trait AppCapture: Send + Sync {
    fn app_name(&self) -> &str;
    fn is_running(&self) -> bool;
    fn capture(&self) -> Option<CaptureEntry>;
    fn watch(&self, engine: Arc<CaptureEngine>);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCaptureAttempt {
    pub app: String,
    pub running: bool,
    pub captured: Option<CID>,
    pub reason: Option<String>,
}

pub struct AppCaptureRegistry {
    handlers: Vec<Box<dyn AppCapture>>,
}

impl AppCaptureRegistry {
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    pub fn add(&mut self, handler: Box<dyn AppCapture>) {
        self.handlers.push(handler);
    }

    pub fn handlers(&self) -> &[Box<dyn AppCapture>] {
        &self.handlers
    }

    pub async fn capture_running(&self, engine: Arc<CaptureEngine>) -> Vec<CID> {
        self.capture_running_detailed(engine)
            .await
            .into_iter()
            .filter_map(|attempt| attempt.captured)
            .collect()
    }

    pub async fn capture_running_detailed(
        &self,
        engine: Arc<CaptureEngine>,
    ) -> Vec<AppCaptureAttempt> {
        let mut attempts = Vec::new();
        for handler in &self.handlers {
            let app = handler.app_name().to_string();
            if !handler.is_running() {
                attempts.push(AppCaptureAttempt {
                    app,
                    running: false,
                    captured: None,
                    reason: Some("not running".to_string()),
                });
                continue;
            }

            let Some(entry) = handler.capture() else {
                attempts.push(AppCaptureAttempt {
                    app,
                    running: true,
                    captured: None,
                    reason: Some("no capturable state found".to_string()),
                });
                continue;
            };

            match engine.capture(entry).await {
                Ok(cid) => attempts.push(AppCaptureAttempt {
                    app,
                    running: true,
                    captured: Some(cid),
                    reason: None,
                }),
                Err(err) => attempts.push(AppCaptureAttempt {
                    app,
                    running: true,
                    captured: None,
                    reason: Some(format!("store failed: {err}")),
                }),
            }
        }
        attempts
    }
}

impl Default for AppCaptureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub fn default_registry(engine: Arc<CaptureEngine>) -> AppCaptureRegistry {
    let mut registry = AppCaptureRegistry::new();
    registry.add(Box::new(ClaudeDesktopCapture));
    registry.add(Box::new(VSCodeCapture));
    registry.add(Box::new(BrowserCapture {
        browser: "Chrome".to_string(),
    }));
    registry.add(Box::new(BrowserCapture {
        browser: "Firefox".to_string(),
    }));

    for handler in registry.handlers() {
        if handler.is_running() {
            handler.watch(engine.clone());
            println!("monitoring app: {}", handler.app_name());
        }
    }

    registry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureKind, CaptureSource};
    use crate::store::CaptureStore;
    use aeon_store::CIDStore;
    use std::path::PathBuf;
    use tokio::sync::Mutex;

    struct FakeCapture {
        app: &'static str,
        running: bool,
        entry: bool,
    }

    impl AppCapture for FakeCapture {
        fn app_name(&self) -> &str {
            self.app
        }

        fn is_running(&self) -> bool {
            self.running
        }

        fn capture(&self) -> Option<CaptureEntry> {
            self.entry.then(|| {
                CaptureEntry::new(
                    format!("state for {}", self.app).into_bytes(),
                    CaptureKind::Text,
                    CaptureSource::AppApi {
                        app: self.app.to_string(),
                    },
                )
            })
        }

        fn watch(&self, _engine: Arc<CaptureEngine>) {}
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-app-capture-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn detailed_capture_reports_success_and_skips() {
        let dir = temp_dir();
        let store = CaptureStore::new(
            CIDStore::new(dir.join("store")).unwrap(),
            dir.join("index.json"),
        )
        .unwrap();
        let engine = Arc::new(CaptureEngine::new(Arc::new(Mutex::new(store))));

        let mut registry = AppCaptureRegistry::new();
        registry.add(Box::new(FakeCapture {
            app: "Running",
            running: true,
            entry: true,
        }));
        registry.add(Box::new(FakeCapture {
            app: "Empty",
            running: true,
            entry: false,
        }));
        registry.add(Box::new(FakeCapture {
            app: "Stopped",
            running: false,
            entry: true,
        }));

        let attempts = registry.capture_running_detailed(engine).await;

        assert_eq!(attempts.len(), 3);
        assert!(attempts[0].captured.is_some());
        assert_eq!(
            attempts[1].reason.as_deref(),
            Some("no capturable state found")
        );
        assert_eq!(attempts[2].reason.as_deref(), Some("not running"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
