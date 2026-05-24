use super::util::process_exists;
use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

const MAX_HISTORY_CHARS: usize = 80_000;

pub struct TerminalCapture;

impl AppCapture for TerminalCapture {
    fn app_name(&self) -> &str {
        "Terminal"
    }

    fn is_running(&self) -> bool {
        terminal_processes()
            .iter()
            .any(|process| process_exists(process))
            || terminal_history_files()
                .iter()
                .any(|(_, path)| path.exists())
    }

    fn capture(&self) -> Option<CaptureEntry> {
        capture_terminal_state()
    }

    fn watch(&self, _engine: Arc<CaptureEngine>) {}
}

pub fn capture_terminal_state() -> Option<CaptureEntry> {
    let running = running_terminal_processes();
    let histories = read_terminal_histories();
    if running.is_empty() && histories.is_empty() {
        return None;
    }

    let mut text = format!("# Terminal state\n\nCaptured: {}\n\n", now_ms());
    if running.is_empty() {
        text.push_str("## Running terminals\n\nNone detected.\n\n");
    } else {
        text.push_str("## Running terminals\n\n");
        for terminal in &running {
            text.push_str("- ");
            text.push_str(&terminal.name);
            if let Some(pid) = terminal.pid {
                text.push_str(&format!(" (PID {pid})"));
            }
            text.push('\n');
            if let Some(parent_pid) = terminal.parent_pid {
                text.push_str(&format!("  Parent PID: {parent_pid}\n"));
            }
            if let Some(path) = terminal.executable_path.as_deref() {
                text.push_str("  Executable: ");
                text.push_str(path);
                text.push('\n');
            }
            if let Some(command_line) = terminal.command_line.as_deref() {
                text.push_str("  Command line: ");
                text.push_str(command_line);
                text.push('\n');
            }
        }
        text.push('\n');
    }

    if histories.is_empty() {
        text.push_str("## Command history\n\nNo persisted history files found.\n");
    } else {
        text.push_str("## Command history\n\n");
        for history in &histories {
            text.push_str("### ");
            text.push_str(&history.label);
            text.push('\n');
            text.push_str("Path: ");
            text.push_str(&history.path);
            text.push_str("\n\n```text\n");
            text.push_str(&history.content);
            text.push_str("\n```\n\n");
        }
    }

    let mut entry = CaptureEntry::new(
        text.into_bytes(),
        CaptureKind::Text,
        CaptureSource::AppApi {
            app: "Terminal".to_string(),
        },
    )
    .with_title("Terminal state")
    .with_summary("Running terminal processes and recent command history")
    .with_app("Terminal");
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "terminal-state".to_string());
    entry.meta.extra.insert(
        "history_file_count".to_string(),
        histories.len().to_string(),
    );
    entry.meta.extra.insert(
        "running_terminal_count".to_string(),
        running.len().to_string(),
    );
    Some(entry)
}

struct TerminalHistory {
    label: String,
    path: String,
    content: String,
}

fn terminal_processes() -> [&'static str; 15] {
    [
        "WindowsTerminal.exe",
        "wt.exe",
        "powershell.exe",
        "pwsh.exe",
        "cmd.exe",
        "bash.exe",
        "mintty.exe",
        "wezterm-gui.exe",
        "gnome-terminal",
        "konsole",
        "kitty",
        "alacritty",
        "wezterm",
        "xterm",
        "ghostty",
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunningTerminal {
    name: String,
    pid: Option<u32>,
    parent_pid: Option<u32>,
    executable_path: Option<String>,
    command_line: Option<String>,
}

fn running_terminal_processes() -> Vec<RunningTerminal> {
    let detailed = windows_terminal_processes();
    if !detailed.is_empty() {
        return detailed;
    }

    let unix = unix_terminal_processes();
    if !unix.is_empty() {
        return unix;
    }

    terminal_processes()
        .iter()
        .filter(|process| process_exists(process))
        .map(|process| RunningTerminal {
            name: (*process).to_string(),
            pid: None,
            parent_pid: None,
            executable_path: None,
            command_line: None,
        })
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn unix_terminal_processes() -> Vec<RunningTerminal> {
    let mut rows = Vec::new();
    for name in terminal_processes() {
        let output = Command::new("pgrep").args(["-x", name]).output().ok();
        let Some(output) = output else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let pid = line.trim().parse::<u32>().ok();
            rows.push(RunningTerminal {
                name: name.to_string(),
                pid,
                parent_pid: None,
                executable_path: None,
                command_line: None,
            });
        }
    }
    rows
}

#[cfg(target_os = "windows")]
fn unix_terminal_processes() -> Vec<RunningTerminal> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn windows_terminal_processes() -> Vec<RunningTerminal> {
    let names = terminal_processes()
        .iter()
        .map(|name| format!("'{name}'"))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!(
        "$names=@({names}); Get-CimInstance Win32_Process | \
         Where-Object {{ $names -contains $_.Name }} | \
         Select-Object ProcessId,ParentProcessId,Name,ExecutablePath,CommandLine | \
         ConvertTo-Json -Compress"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .output()
        .ok();
    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_windows_terminal_processes(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(not(target_os = "windows"))]
fn windows_terminal_processes() -> Vec<RunningTerminal> {
    Vec::new()
}

#[derive(Debug, Deserialize)]
struct WindowsProcessRow {
    #[serde(rename = "ProcessId")]
    process_id: Option<u32>,
    #[serde(rename = "ParentProcessId")]
    parent_process_id: Option<u32>,
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "ExecutablePath")]
    executable_path: Option<String>,
    #[serde(rename = "CommandLine")]
    command_line: Option<String>,
}

fn parse_windows_terminal_processes(json: &str) -> Vec<RunningTerminal> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json.trim()) else {
        return Vec::new();
    };
    let rows = match value {
        serde_json::Value::Array(rows) => rows,
        serde_json::Value::Object(_) => vec![value],
        _ => Vec::new(),
    };

    rows.into_iter()
        .filter_map(|value| serde_json::from_value::<WindowsProcessRow>(value).ok())
        .filter_map(|row| {
            let name = row.name?;
            Some(RunningTerminal {
                name,
                pid: row.process_id,
                parent_pid: row.parent_process_id,
                executable_path: row.executable_path.filter(|value| !value.trim().is_empty()),
                command_line: row.command_line.filter(|value| !value.trim().is_empty()),
            })
        })
        .collect()
}

fn terminal_history_files() -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    if let Some(data_dir) = dirs::data_dir() {
        files.push((
            "PowerShell PSReadLine".to_string(),
            data_dir
                .join("Microsoft")
                .join("Windows")
                .join("PowerShell")
                .join("PSReadLine")
                .join("ConsoleHost_history.txt"),
        ));
    }
    if let Some(home) = dirs::home_dir() {
        files.push(("Git Bash".to_string(), home.join(".bash_history")));
        files.push(("Zsh".to_string(), home.join(".zsh_history")));
        files.push(("Python REPL".to_string(), home.join(".python_history")));
        files.push(("Node REPL".to_string(), home.join(".node_repl_history")));
    }
    files
}

fn read_terminal_histories() -> Vec<TerminalHistory> {
    terminal_history_files()
        .into_iter()
        .filter_map(|(label, path)| {
            let content = std::fs::read_to_string(&path).ok()?;
            let content = tail_chars(&content, MAX_HISTORY_CHARS);
            Some(TerminalHistory {
                label,
                path: path.display().to_string(),
                content,
            })
        })
        .collect()
}

fn tail_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    text.chars().skip(count - max_chars).collect()
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

    #[test]
    fn parses_windows_terminal_process_array() {
        let rows = parse_windows_terminal_processes(
            r#"[{"ProcessId":12,"ParentProcessId":8,"Name":"powershell.exe","ExecutablePath":"C:\\PowerShell\\powershell.exe","CommandLine":"powershell -NoLogo"}]"#,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "powershell.exe");
        assert_eq!(rows[0].pid, Some(12));
        assert_eq!(rows[0].parent_pid, Some(8));
        assert_eq!(rows[0].command_line.as_deref(), Some("powershell -NoLogo"));
    }

    #[test]
    fn parses_single_windows_terminal_process_object() {
        let rows = parse_windows_terminal_processes(
            r#"{"ProcessId":42,"ParentProcessId":1,"Name":"cmd.exe","ExecutablePath":null,"CommandLine":"cmd.exe"}"#,
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "cmd.exe");
        assert_eq!(rows[0].pid, Some(42));
    }
}
