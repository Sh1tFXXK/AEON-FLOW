use super::*;

use super::shared::*;

#[derive(Deserialize)]
pub struct CaptureProcessRequest {
    pub pid: u32,
    pub option_id: String,
    pub target_device: Option<String>,
}

pub async fn capture_apps(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (captured, attempts) = capture_known_apps(&state).await;
    Json(serde_json::json!({
        "captured": captured,
        "attempts": attempts,
    }))
}

pub async fn capture_processes(State(state): State<AppState>) -> Json<serde_json::Value> {
    match capture_process_inventory(&state).await {
        Ok((cid, count)) => Json(serde_json::json!({
            "ok": true,
            "cid": hex_cid(&cid),
            "captured": [hex_cid(&cid)],
            "process_count": count,
            "relay": state.relay_url.is_some(),
            "relay_space": state.relay_space.clone(),
            "message": format!("captured {count} running processes"),
        })),
        Err(error) => Json(serde_json::json!({"ok": false, "error": error})),
    }
}

pub async fn capture_all(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut captured = Vec::new();
    let mut errors = Vec::new();
    let process_count = match capture_process_inventory(&state).await {
        Ok((cid, count)) => {
            captured.push(hex_cid(&cid));
            count
        }
        Err(error) => {
            errors.push(error);
            0
        }
    };
    let (app_captured, attempts) = capture_known_apps(&state).await;
    captured.extend(app_captured);
    let (window_captured, window_attempts) = capture_visible_app_windows(&state).await;
    captured.extend(window_captured);

    Json(serde_json::json!({
        "ok": errors.is_empty() || !captured.is_empty(),
        "captured": captured,
        "process_count": process_count,
        "attempts": attempts,
        "window_attempts": window_attempts,
        "errors": errors,
        "relay": state.relay_url.is_some(),
        "relay_space": state.relay_space.clone(),
        "message": "full machine state captured",
    }))
}

async fn capture_known_apps(state: &AppState) -> (Vec<String>, Vec<serde_json::Value>) {
    let mut captured = Vec::new();
    let mut attempts = Vec::new();

    for handler in state.app_registry.handlers() {
        let app = handler.app_name().to_string();
        if !handler.is_running() {
            attempts.push(serde_json::json!({
                "app": app,
                "running": false,
                "captured": null,
                "reason": "not running",
            }));
            continue;
        }

        let mut entry = match aeon_capture::apps::capture_app_entry(handler.as_ref()) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                attempts.push(serde_json::json!({
                    "app": app,
                    "running": true,
                    "captured": null,
                    "reason": "no capturable state found",
                }));
                continue;
            }
            Err(reason) => {
                attempts.push(serde_json::json!({
                    "app": app,
                    "running": true,
                    "captured": null,
                    "reason": reason,
                }));
                continue;
            }
        };

        stamp_capture_identity(&mut entry, state);
        match state.capture_engine.capture(entry).await {
            Ok(cid) => {
                let hex = hex_cid(&cid);
                captured.push(hex.clone());
                attempts.push(serde_json::json!({
                    "app": app,
                    "running": true,
                    "captured": hex,
                    "reason": null,
                }));
            }
            Err(err) => attempts.push(serde_json::json!({
                "app": app,
                "running": true,
                "captured": null,
                "reason": format!("store failed: {err}"),
            })),
        }
    }

    (captured, attempts)
}

async fn capture_visible_app_windows(state: &AppState) -> (Vec<String>, Vec<serde_json::Value>) {
    let windows = tokio::task::spawn_blocking(aeon_capture::screenshot::list_visible_windows)
        .await
        .unwrap_or_default();
    let mut seen = HashSet::new();
    let mut captured = Vec::new();
    let mut attempts = Vec::new();

    for window in windows {
        if captured.len() >= MAX_VISIBLE_APP_CAPTURES {
            break;
        }
        if !seen.insert(window.pid) {
            continue;
        }
        let process_name = crate::process::process_name(window.pid)
            .unwrap_or_else(|| format!("pid-{}", window.pid));
        if is_ignored_window_process(&process_name) {
            continue;
        }
        match super::process_helpers::capture_generic_app_state(window.pid, state).await {
            Ok(value) => {
                if let Some(cid) = value.get("cid").and_then(|cid| cid.as_str()) {
                    captured.push(cid.to_string());
                }
                attempts.push(serde_json::json!({
                    "pid": window.pid,
                    "title": window.title,
                    "process": process_name,
                    "captured": value.get("cid").and_then(|cid| cid.as_str()),
                    "screenshot": value.get("screenshot_cid").and_then(|cid| cid.as_str()),
                    "reason": null,
                }));
            }
            Err(error) => attempts.push(serde_json::json!({
                "pid": window.pid,
                "title": window.title,
                "process": process_name,
                "captured": null,
                "reason": error,
            })),
        }
    }

    (captured, attempts)
}

fn is_ignored_window_process(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "explorer.exe"
            | "searchhost.exe"
            | "startmenuexperiencehost.exe"
            | "shellexperiencehost.exe"
            | "applicationframehost.exe"
    )
}

async fn capture_process_inventory(state: &AppState) -> Result<(CID, usize), String> {
    let processes = tokio::task::spawn_blocking(crate::process::list_processes)
        .await
        .map_err(|err| err.to_string())?;
    let count = processes.len();
    let payload = serde_json::json!({
        "captured_at": now_ms(),
        "process_count": count,
        "processes": processes,
    });
    let data = serde_json::to_vec_pretty(&payload).map_err(|err| err.to_string())?;
    let mut entry = CaptureEntry::new(data, CaptureKind::ProcessState, CaptureSource::Manual)
        .with_title(&format!("Process inventory ({count})"))
        .with_summary(&format!("Captured {count} running processes"))
        .with_app("Processes");
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "process-inventory".to_string());
    entry
        .meta
        .extra
        .insert("process_count".to_string(), count.to_string());
    stamp_capture_identity(&mut entry, state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|err| err.to_string())?;
    Ok((cid, count))
}

pub async fn list_process_entries() -> Json<Vec<crate::process::ProcessInfo>> {
    let processes = tokio::task::spawn_blocking(crate::process::list_processes)
        .await
        .unwrap_or_default();
    Json(processes)
}

pub async fn list_vm_entries() -> Json<Vec<AeonVmInfo>> {
    let vms = tokio::task::spawn_blocking(|| list_recent_vms(240))
        .await
        .unwrap_or_default();
    Json(vms)
}

pub async fn capture_process(
    Path(pid): Path<u32>,
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let capture = ProcessStateCapture { pid };
    let Some(mut entry) = capture.capture() else {
        return Err(StatusCode::NOT_FOUND);
    };
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(serde_json::json!({"ok": true, "cid": hex_cid(&cid)})))
}

pub async fn capture_process_option(
    State(state): State<AppState>,
    Json(req): Json<CaptureProcessRequest>,
) -> Json<serde_json::Value> {
    Json(super::process_helpers::execute_capture_option(req, &state).await)
}
