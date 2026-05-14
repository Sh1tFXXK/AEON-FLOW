use super::util::{find_latest_file, process_exists};
use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::Arc;

pub struct ClaudeDesktopCapture;

impl AppCapture for ClaudeDesktopCapture {
    fn app_name(&self) -> &str {
        "Claude Desktop"
    }

    fn is_running(&self) -> bool {
        process_exists("Claude.exe")
    }

    fn capture(&self) -> Option<CaptureEntry> {
        let claude_dir = dirs::data_dir()?.join("Claude");
        let latest = find_latest_file(&claude_dir, "json")?;
        let data = std::fs::read(&latest).ok()?;
        let conv: serde_json::Value = serde_json::from_slice(&data).unwrap_or_default();
        let msg_count = conv["messages"]
            .as_array()
            .map(|msgs| msgs.len())
            .unwrap_or(0);

        let mut entry = CaptureEntry::new(
            data,
            CaptureKind::Conversation,
            CaptureSource::AppApi {
                app: "Claude Desktop".to_string(),
            },
        );
        entry.meta.message_count = Some(msg_count);
        entry.meta.app_name = Some("Claude Desktop".to_string());
        entry.meta.title = Some(format!("Claude 对话（{msg_count} 条消息）"));
        entry.meta.file_path = Some(latest.to_string_lossy().to_string());
        Some(entry)
    }

    fn watch(&self, engine: Arc<CaptureEngine>) {
        let Some(claude_dir) = dirs::data_dir().map(|dir| dir.join("Claude")) else {
            return;
        };
        if !claude_dir.exists() {
            return;
        }

        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel();
            let Ok(mut watcher) = RecommendedWatcher::new(tx, Config::default()) else {
                return;
            };
            if watcher
                .watch(&claude_dir, RecursiveMode::Recursive)
                .is_err()
            {
                return;
            }
            for event in rx.into_iter().flatten() {
                if event.paths.iter().any(|path| {
                    path.extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                }) {
                    let engine = engine.clone();
                    handle.spawn(async move {
                        let capture = ClaudeDesktopCapture;
                        if let Some(entry) = capture.capture() {
                            let _ = engine.capture(entry).await;
                        }
                    });
                }
            }
        });
    }
}
