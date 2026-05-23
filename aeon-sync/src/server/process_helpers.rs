use super::process_capture::CaptureProcessRequest;
use super::shared::*;
use super::*;

pub(super) async fn execute_capture_option(
    req: CaptureProcessRequest,
    state: &AppState,
) -> serde_json::Value {
    let result = match req.option_id.as_str() {
        id if id.starts_with("screenshot") => capture_window_screenshot(req.pid, state).await,
        "claude_conversation" => {
            capture_app_entry(
                ClaudeDesktopCapture,
                state,
                "Claude conversation captured to AEON",
            )
            .await
        }
        "vscode_workspace" | "vscode_current_file" => {
            capture_app_entry(VSCodeCapture, state, "VS Code workspace captured").await
        }
        "browser_tab" => capture_browser_tab(req.pid, state).await,
        "browser_pages" => capture_browser_pages_option(req.pid, state).await,
        "browser_bookmarks" => capture_chrome_bookmarks(state).await,
        "terminal_state" => capture_terminal_state_option(state).await,
        "obsidian_vault" => {
            capture_process_metadata(req.pid, state, Some("Obsidian vault outline")).await
        }
        "metadata" => capture_process_metadata(req.pid, state, None).await,
        id if id.starts_with("metadata_") => capture_process_metadata(req.pid, state, None).await,
        id if id.starts_with("app_state_") => capture_generic_app_state(req.pid, state).await,
        id if id.starts_with("snapshot_") => {
            let vm_id = id.trim_start_matches("snapshot_");
            capture_vm_action(vm_id, state, None, "VM snapshot captured").await
        }
        id if id.starts_with("migrate_") => {
            let vm_id = id.trim_start_matches("migrate_");
            let target = req.target_device.as_deref().unwrap_or("aeon-relay");
            capture_vm_action(vm_id, state, Some(target), "migration snapshot created").await
        }
        id if id.starts_with("pause_") => {
            let vm_id = id.trim_start_matches("pause_");
            match set_vm_status(vm_id, "paused") {
                Ok(_) => {
                    capture_vm_action(vm_id, state, None, "VM paused and snapshot captured").await
                }
                Err(err) => Err(err),
            }
        }
        _ => Err("unknown capture action".to_string()),
    };

    match result {
        Ok(value) => value,
        Err(error) => serde_json::json!({"ok": false, "error": error}),
    }
}

async fn capture_app_entry<T>(
    capture: T,
    state: &AppState,
    message: &str,
) -> Result<serde_json::Value, String>
where
    T: AppCapture,
{
    let mut entry = aeon_capture::apps::capture_app_entry(&capture)
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "no capturable app state found".to_string())?;
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": message,
    }))
}

async fn capture_browser_tab(pid: u32, state: &AppState) -> Result<serde_json::Value, String> {
    let name = crate::process::process_name(pid).unwrap_or_default();
    let browser = browser_name_from_process(&name);
    capture_app_entry(
        BrowserCapture {
            browser: browser.to_string(),
        },
        state,
        "browser tab captured",
    )
    .await
}

async fn capture_browser_pages_option(
    pid: u32,
    state: &AppState,
) -> Result<serde_json::Value, String> {
    let name = crate::process::process_name(pid).unwrap_or_default();
    let browser = browser_name_from_process(&name);
    let mut entry = tokio::task::spawn_blocking(move || capture_browser_pages(browser, 30))
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "no browser page history found".to_string())?;
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "browser page list captured",
    }))
}

fn browser_name_from_process(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("firefox") {
        "Firefox"
    } else if lower.contains("edge") || lower.contains("msedge") {
        "Edge"
    } else {
        "Chrome"
    }
}

async fn capture_terminal_state_option(state: &AppState) -> Result<serde_json::Value, String> {
    let mut entry = tokio::task::spawn_blocking(capture_terminal_state)
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "no terminal history or running terminal found".to_string())?;
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "terminal state captured",
    }))
}

async fn capture_chrome_bookmarks(state: &AppState) -> Result<serde_json::Value, String> {
    let path = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA not found".to_string())?
        .join("Google")
        .join("Chrome")
        .join("User Data")
        .join("Default")
        .join("Bookmarks");
    let data = tokio::fs::read(&path)
        .await
        .map_err(|err| format!("read bookmarks failed: {err}"))?;
    let mut entry = CaptureEntry::new(
        data,
        CaptureKind::Document {
            format: "json".to_string(),
        },
        CaptureSource::AppApi {
            app: "Chrome".to_string(),
        },
    )
    .with_title("Chrome bookmarks");
    entry.meta.file_path = Some(path.to_string_lossy().to_string());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "chrome-bookmarks".to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "Chrome bookmarks captured",
    }))
}

async fn capture_window_screenshot(
    pid: u32,
    state: &AppState,
) -> Result<serde_json::Value, String> {
    let (data, width, height) = tokio::task::spawn_blocking(move || {
        aeon_capture::screenshot::capture_window_screenshot_bytes(pid)
    })
    .await
    .map_err(|err| err.to_string())??;
    let process_name =
        crate::process::process_name(pid).unwrap_or_else(|| format!("process-{pid}"));
    let mut entry = CaptureEntry::new(
        data,
        CaptureKind::Image {
            width,
            height,
            format: "png".to_string(),
        },
        CaptureSource::Screenshot,
    )
    .with_title(&format!("{process_name} screenshot"));
    entry.meta.extra.insert("pid".to_string(), pid.to_string());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "window-screenshot".to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "screenshot captured",
    }))
}

pub(super) async fn capture_generic_app_state(
    pid: u32,
    state: &AppState,
) -> Result<serde_json::Value, String> {
    let metadata =
        crate::process::process_metadata(pid).ok_or_else(|| "process not found".to_string())?;
    let process_name = metadata
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("process")
        .to_string();
    let windows = tokio::task::spawn_blocking(aeon_capture::screenshot::list_visible_windows)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|window| window.pid == pid)
        .collect::<Vec<_>>();

    let screenshot = capture_window_screenshot(pid, state).await.ok();
    let screenshot_cid = screenshot
        .as_ref()
        .and_then(|value| value.get("cid"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);

    let payload = serde_json::json!({
        "capture_mode": "generic-application-state",
        "captured_at": now_ms(),
        "process": metadata,
        "windows": windows,
        "screenshot_cid": screenshot_cid,
    });
    let data = serde_json::to_vec_pretty(&payload).map_err(|err| err.to_string())?;
    let mut entry = CaptureEntry::new(data, CaptureKind::ProcessState, CaptureSource::Manual)
        .with_title(&format!("{process_name} application state"))
        .with_summary(&format!(
            "Captured process metadata{} for PID {pid}",
            if screenshot_cid.is_some() {
                " and window screenshot"
            } else {
                ""
            }
        ))
        .with_app(&process_name);
    entry.meta.extra.insert("pid".to_string(), pid.to_string());
    entry.meta.extra.insert(
        "capture_mode".to_string(),
        "generic-application-state".to_string(),
    );
    if let Some(cid) = &screenshot_cid {
        entry
            .meta
            .extra
            .insert("screenshot_cid".to_string(), cid.clone());
    }
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "screenshot_cid": screenshot_cid,
        "message": "application state captured",
    }))
}

async fn capture_process_metadata(
    pid: u32,
    state: &AppState,
    title: Option<&str>,
) -> Result<serde_json::Value, String> {
    let metadata =
        crate::process::process_metadata(pid).ok_or_else(|| "process not found".to_string())?;
    let data = serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())?;
    let name = metadata
        .get("name")
        .and_then(|value| value.as_str())
        .unwrap_or("process");
    let title = title
        .map(str::to_string)
        .unwrap_or_else(|| format!("{name} process info"));
    let mut entry = CaptureEntry::new(data, CaptureKind::ProcessState, CaptureSource::Manual)
        .with_title(&title)
        .with_app("Process");
    entry.meta.extra.insert("pid".to_string(), pid.to_string());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "process-metadata".to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": "process info captured",
    }))
}

async fn capture_vm_action(
    vm_id: &str,
    state: &AppState,
    target_device: Option<&str>,
    message: &str,
) -> Result<serde_json::Value, String> {
    let mut entry = capture_vm_snapshot(vm_id)?;
    let transfer_target = target_device.filter(|target| !target.trim().is_empty());
    if transfer_target.is_some() && state.relay_url.is_none() {
        return Err(
            "AEON Relay is not enabled; start with scripts\\aeon.ps1 to transfer VM snapshots"
                .to_string(),
        );
    }
    if let Some(target) = transfer_target {
        entry.meta.title = Some(format!("VM migration snapshot {vm_id} -> {target}"));
        entry
            .meta
            .extra
            .insert("migration_target".to_string(), target.to_string());
        entry
            .meta
            .extra
            .insert("transfer_mode".to_string(), "aeon-relay".to_string());
    }
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok(serde_json::json!({
        "ok": true,
        "cid": hex_cid(&cid),
        "message": message,
        "vm_id": vm_id,
        "target": transfer_target,
        "relay": state.relay_url.is_some(),
        "relay_space": state.relay_space.clone(),
    }))
}

pub async fn capture_vm(
    Path(vm_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let mut entry = capture_vm_snapshot(&vm_id).map_err(|err| {
        tracing::warn!("capture vm {vm_id} failed: {err}");
        StatusCode::NOT_FOUND
    })?;
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true, "cid": hex_cid(&cid)})))
}
