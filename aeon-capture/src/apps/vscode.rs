use super::util::{find_latest_file, now_ms, process_exists};
use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use std::path::PathBuf;
use std::sync::Arc;

pub struct VSCodeCapture;

impl AppCapture for VSCodeCapture {
    fn app_name(&self) -> &str {
        "VS Code"
    }

    fn is_running(&self) -> bool {
        process_exists("Code.exe")
    }

    fn capture(&self) -> Option<CaptureEntry> {
        let workspace = find_vscode_workspace()?;
        let workspace_file = find_latest_file(&workspace, "json");
        let workspace_state = workspace_file
            .as_ref()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .unwrap_or_default();
        let workspace_name = workspace
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("workspace");

        let data = serde_json::to_vec(&serde_json::json!({
            "workspace": workspace.to_string_lossy(),
            "workspace_state_file": workspace_file.map(|path| path.to_string_lossy().to_string()),
            "state_preview": workspace_state.chars().take(4000).collect::<String>(),
            "captured_at": now_ms(),
        }))
        .ok()?;

        let mut entry = CaptureEntry::new(
            data,
            CaptureKind::Code {
                language: "workspace".to_string(),
            },
            CaptureSource::AppApi {
                app: "VS Code".to_string(),
            },
        )
        .with_title(&format!("VS Code 工作区: {workspace_name}"));
        entry.meta.app_name = Some("VS Code".to_string());
        entry.meta.file_path = Some(workspace.to_string_lossy().to_string());
        Some(entry)
    }

    fn watch(&self, _engine: Arc<CaptureEngine>) {}
}

fn find_vscode_workspace() -> Option<PathBuf> {
    let storage = dirs::data_dir()?
        .join("Code")
        .join("User")
        .join("workspaceStorage");
    let mut latest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(storage).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let modified = entry.metadata().ok()?.modified().ok()?;
        if latest
            .as_ref()
            .is_none_or(|(current, _)| modified > *current)
        {
            latest = Some((modified, path));
        }
    }
    latest.map(|(_, path)| path)
}
