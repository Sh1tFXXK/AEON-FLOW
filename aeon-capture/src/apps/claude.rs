use super::util::process_exists;
use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Component, Path, PathBuf};
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
        let (latest, data, msg_count) = find_latest_conversation(&claude_dir)?;

        let mut entry = CaptureEntry::new(
            data,
            CaptureKind::Conversation,
            CaptureSource::AppApi {
                app: "Claude Desktop".to_string(),
            },
        );
        entry.meta.message_count = Some(msg_count);
        entry.meta.app_name = Some("Claude Desktop".to_string());
        entry.meta.title = Some(format!("Claude conversation ({msg_count} messages)"));
        entry.meta.file_path = Some(latest.to_string_lossy().to_string());
        entry.meta.extra.insert(
            "capture_mode".to_string(),
            "claude-conversation-json".to_string(),
        );
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
                if event
                    .paths
                    .iter()
                    .any(|path| is_conversation_candidate(path))
                {
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

fn find_latest_conversation(root: &Path) -> Option<(PathBuf, Vec<u8>, usize)> {
    let mut latest: Option<(std::time::SystemTime, PathBuf, Vec<u8>, usize)> = None;
    collect_conversation_files(root, &mut latest);
    latest.map(|(_, path, data, count)| (path, data, count))
}

fn collect_conversation_files(
    dir: &Path,
    latest: &mut Option<(std::time::SystemTime, PathBuf, Vec<u8>, usize)>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_conversation_files(&path, latest);
            continue;
        }
        if !is_conversation_candidate(&path) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if latest
            .as_ref()
            .is_some_and(|(current, _, _, _)| modified <= *current)
        {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let Some(message_count) = conversation_message_count(&data) else {
            continue;
        };
        latest.replace((modified, path, data, message_count));
    }
}

fn is_conversation_candidate(path: &Path) -> bool {
    if !path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
    {
        return false;
    }

    !path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        let name = name.to_string_lossy().to_ascii_lowercase();
        matches!(
            name.as_str(),
            "sentry"
                | "queue"
                | "config.json"
                | "window-state.json"
                | "extensions-blocklist.json"
                | "claude_desktop_config.json"
                | "developer_settings.json"
        )
    })
}

fn conversation_message_count(data: &[u8]) -> Option<usize> {
    let value: serde_json::Value = serde_json::from_slice(data).ok()?;
    let count = value
        .get("messages")
        .and_then(|messages| messages.as_array())
        .map(Vec::len)?;
    (count > 0).then_some(count)
}
