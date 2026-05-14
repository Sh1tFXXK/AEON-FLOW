use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub struct ProcessStateCapture {
    pub pid: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: u32,
    pub image_name: String,
    pub session_name: Option<String>,
    pub session_number: Option<String>,
    pub memory_usage: Option<String>,
}

impl AppCapture for ProcessStateCapture {
    fn app_name(&self) -> &str {
        "Process State"
    }

    fn is_running(&self) -> bool {
        process_snapshot(self.pid).is_some()
    }

    fn capture(&self) -> Option<CaptureEntry> {
        let snapshot = process_snapshot(self.pid)?;
        let data = serde_json::to_vec(&snapshot).ok()?;
        let title = snapshot
            .get("image_name")
            .and_then(|value| value.as_str())
            .map(|name| format!("{name} ({})", self.pid))
            .unwrap_or_else(|| format!("Process State {}", self.pid));
        let mut entry = CaptureEntry::new(data, CaptureKind::ProcessState, CaptureSource::Manual)
            .with_title(&title)
            .with_app("Process");
        entry
            .meta
            .extra
            .insert("pid".to_string(), self.pid.to_string());
        entry
            .meta
            .extra
            .insert("capture_mode".to_string(), "process-metadata".to_string());
        Some(entry)
    }

    fn watch(&self, _engine: Arc<CaptureEngine>) {}
}

pub fn list_processes() -> Vec<RunningProcess> {
    let mut processes = platform_processes();
    processes.sort_by(|a, b| {
        a.image_name
            .to_ascii_lowercase()
            .cmp(&b.image_name.to_ascii_lowercase())
            .then(a.pid.cmp(&b.pid))
    });
    processes
}

#[cfg(windows)]
fn process_snapshot(pid: u32) -> Option<serde_json::Value> {
    let process = list_processes()
        .into_iter()
        .find(|process| process.pid == pid)?;
    Some(serde_json::json!({
        "pid": pid,
        "image_name": process.image_name,
        "session_name": process.session_name,
        "session_number": process.session_number,
        "memory_usage": process.memory_usage,
        "capture_mode": "tasklist-metadata"
    }))
}

#[cfg(windows)]
fn platform_processes() -> Vec<RunningProcess> {
    let Ok(output) = std::process::Command::new("tasklist.exe")
        .args(["/FO", "CSV", "/NH"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_tasklist_row)
        .collect()
}

#[cfg(windows)]
fn parse_tasklist_row(line: &str) -> Option<RunningProcess> {
    let columns = parse_tasklist_csv_line(line);
    if columns.len() < 5 {
        return None;
    }
    let pid = columns[1].parse().ok()?;
    Some(RunningProcess {
        pid,
        image_name: columns[0].clone(),
        session_name: Some(columns[2].clone()),
        session_number: Some(columns[3].clone()),
        memory_usage: Some(columns[4].clone()),
    })
}

#[cfg(windows)]
fn parse_tasklist_csv_line(line: &str) -> Vec<String> {
    let mut cols = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                current.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                cols.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    cols.push(current);
    cols
}

#[cfg(not(windows))]
fn process_snapshot(pid: u32) -> Option<serde_json::Value> {
    let process = list_processes()
        .into_iter()
        .find(|process| process.pid == pid)?;
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    Some(serde_json::json!({
        "pid": pid,
        "image_name": process.image_name,
        "memory_usage": process.memory_usage,
        "status": status,
        "capture_mode": "proc-status"
    }))
}

#[cfg(not(windows))]
fn platform_processes() -> Vec<RunningProcess> {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let root = entry.path();
            let image_name = std::fs::read_to_string(root.join("comm"))
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| format!("process-{pid}"));
            let memory_usage =
                std::fs::read_to_string(root.join("status"))
                    .ok()
                    .and_then(|status| {
                        status
                            .lines()
                            .find(|line| line.starts_with("VmRSS:"))
                            .map(|line| line.trim().to_string())
                    });
            Some(RunningProcess {
                pid,
                image_name,
                session_name: None,
                session_number: None,
                memory_usage,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use super::*;

    #[cfg(windows)]
    #[test]
    fn parses_tasklist_csv_with_commas() {
        let line = "\"app, name.exe\",\"1234\",\"Console\",\"1\",\"12,344 K\"";

        let process = parse_tasklist_row(line).unwrap();

        assert_eq!(process.image_name, "app, name.exe");
        assert_eq!(process.pid, 1234);
        assert_eq!(process.memory_usage.as_deref(), Some("12,344 K"));
    }
}
