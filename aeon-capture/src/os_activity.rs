use crate::capture::{CaptureEntry, CaptureKind, CaptureSource, OsCaptureProvider};
use crate::engine::CaptureEngine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{interval, Duration};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowBounds {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForegroundWindow {
    pub pid: u32,
    pub process_name: Option<String>,
    pub title: String,
    pub bounds: Option<WindowBounds>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum InputSensitivity {
    NonSensitive,
    Sensitive,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TextCommit {
    pub text: String,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub control_name: Option<String>,
    pub sensitivity: InputSensitivity,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OsActivity {
    WindowFocus {
        provider: OsCaptureProvider,
        window: ForegroundWindow,
    },
    TextCommit {
        provider: OsCaptureProvider,
        commit: TextCommit,
    },
}

#[derive(Debug, Default)]
pub struct TextCommitTracker {
    last_signature: Option<TextCommitSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextCommitSignature {
    app_name: Option<String>,
    window_title: Option<String>,
    control_name: Option<String>,
    text: String,
}

impl TextCommitTracker {
    pub fn next_activity(
        &mut self,
        commit: TextCommit,
        provider: OsCaptureProvider,
    ) -> Option<OsActivity> {
        if commit.text.trim().is_empty() {
            return None;
        }

        let signature = TextCommitSignature::from(&commit);
        if self.last_signature.as_ref() == Some(&signature) {
            return None;
        }
        self.last_signature = Some(signature);
        Some(OsActivity::text_commit(commit, provider))
    }
}

impl From<&TextCommit> for TextCommitSignature {
    fn from(commit: &TextCommit) -> Self {
        Self {
            app_name: commit.app_name.clone(),
            window_title: commit.window_title.clone(),
            control_name: commit.control_name.clone(),
            text: commit.text.clone(),
        }
    }
}

pub fn parse_text_commit_json(value: &str) -> serde_json::Result<TextCommit> {
    serde_json::from_str(value)
}

impl OsActivity {
    pub fn window_focus(window: ForegroundWindow, provider: OsCaptureProvider) -> Self {
        Self::WindowFocus { provider, window }
    }

    pub fn text_commit(commit: TextCommit, provider: OsCaptureProvider) -> Self {
        Self::TextCommit { provider, commit }
    }

    pub fn into_capture_entry(self) -> Result<CaptureEntry, serde_json::Error> {
        let provider = self.provider();
        let title = self.title();
        let summary = self.summary();
        let app_name = self.app_name();
        let kind_key = self.kind_key();
        let pid = self.pid();
        let stored = self.redacted_for_storage();
        let data = serde_json::to_vec(&stored)?;

        let mut entry = CaptureEntry::new(
            data,
            CaptureKind::OsActivity,
            CaptureSource::OperatingSystem { provider },
        )
        .with_title(&title);
        entry.meta.summary = Some(summary);
        entry.meta.app_name = app_name;
        entry
            .meta
            .extra
            .insert("os_activity_kind".to_string(), kind_key.to_string());
        if let Some(pid) = pid {
            entry.meta.extra.insert("pid".to_string(), pid.to_string());
        }
        Ok(entry)
    }

    fn provider(&self) -> OsCaptureProvider {
        match self {
            Self::WindowFocus { provider, .. } | Self::TextCommit { provider, .. } => *provider,
        }
    }

    fn kind_key(&self) -> &'static str {
        match self {
            Self::WindowFocus { .. } => "window_focus",
            Self::TextCommit { .. } => "text_commit",
        }
    }

    fn pid(&self) -> Option<u32> {
        match self {
            Self::WindowFocus { window, .. } => Some(window.pid),
            Self::TextCommit { .. } => None,
        }
    }

    fn title(&self) -> String {
        match self {
            Self::WindowFocus { window, .. } => format!("Window focus: {}", window.title),
            Self::TextCommit { commit, .. } => commit
                .app_name
                .as_deref()
                .map(|app| format!("Text commit: {app}"))
                .unwrap_or_else(|| "Text commit".to_string()),
        }
    }

    fn summary(&self) -> String {
        match self {
            Self::WindowFocus { window, .. } => window
                .process_name
                .as_deref()
                .map(|process| format!("{process} focused window '{}'", window.title))
                .unwrap_or_else(|| format!("Focused window '{}'", window.title)),
            Self::TextCommit { commit, .. } => match commit.sensitivity {
                InputSensitivity::NonSensitive => commit.text.chars().take(120).collect(),
                InputSensitivity::Sensitive => "Text commit redacted by sensitivity policy".into(),
                InputSensitivity::Unknown => {
                    "Text commit redacted because sensitivity is unknown".into()
                }
            },
        }
    }

    fn app_name(&self) -> Option<String> {
        match self {
            Self::WindowFocus { window, .. } => window.process_name.clone(),
            Self::TextCommit { commit, .. } => commit.app_name.clone(),
        }
    }

    fn redacted_for_storage(self) -> Self {
        match self {
            Self::TextCommit {
                provider,
                mut commit,
            } if commit.sensitivity != InputSensitivity::NonSensitive => {
                commit.text.clear();
                Self::TextCommit { provider, commit }
            }
            other => other,
        }
    }
}

#[cfg(target_os = "windows")]
pub fn current_foreground_window() -> Option<ForegroundWindow> {
    let script = r#"
$ErrorActionPreference = 'Stop'
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class AeonForegroundWindow {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int maxCount);
    public struct RECT { public int Left; public int Top; public int Right; public int Bottom; }
}
"@
$hwnd = [AeonForegroundWindow]::GetForegroundWindow()
if ($hwnd -eq [IntPtr]::Zero) { exit 0 }
$pidValue = [uint32]0
[AeonForegroundWindow]::GetWindowThreadProcessId($hwnd, [ref]$pidValue) | Out-Null
if ($pidValue -eq 0) { exit 0 }
$titleBuilder = New-Object System.Text.StringBuilder 512
[AeonForegroundWindow]::GetWindowTextW($hwnd, $titleBuilder, $titleBuilder.Capacity) | Out-Null
$title = $titleBuilder.ToString().Trim()
if (-not $title) { exit 0 }
$rect = New-Object AeonForegroundWindow+RECT
$bounds = $null
if ([AeonForegroundWindow]::GetWindowRect($hwnd, [ref]$rect)) {
    $bounds = [pscustomobject]@{
        left = $rect.Left
        top = $rect.Top
        width = $rect.Right - $rect.Left
        height = $rect.Bottom - $rect.Top
    }
}
$processName = $null
try {
    $processName = (Get-Process -Id $pidValue -ErrorAction Stop).ProcessName
} catch {}
[pscustomobject]@{
    pid = [uint32]$pidValue
    process_name = $processName
    title = $title
    bounds = $bounds
} | ConvertTo-Json -Depth 4
"#;

    let Ok(output) = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
    else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<ForegroundWindow>(trimmed).ok()
}

#[cfg(not(target_os = "windows"))]
pub fn current_foreground_window() -> Option<ForegroundWindow> {
    None
}

#[cfg(target_os = "windows")]
pub fn current_text_commit() -> Option<TextCommit> {
    let script = r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class AeonForegroundTitle {
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr hWnd, StringBuilder text, int maxCount);
    public static string Read() {
        IntPtr hwnd = GetForegroundWindow();
        if (hwnd == IntPtr.Zero) return null;
        StringBuilder text = new StringBuilder(512);
        GetWindowTextW(hwnd, text, text.Capacity);
        string value = text.ToString().Trim();
        return value.Length == 0 ? null : value;
    }
}
"@
$element = [System.Windows.Automation.AutomationElement]::FocusedElement
if ($null -eq $element) { exit 0 }

$text = $null
$valuePattern = $null
if ($element.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$valuePattern)) {
    try { $text = $valuePattern.Current.Value } catch {}
}

if ([string]::IsNullOrWhiteSpace($text)) {
    $textPattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$textPattern)) {
        try { $text = $textPattern.DocumentRange.GetText(4096) } catch {}
    }
}

if ([string]::IsNullOrWhiteSpace($text)) { exit 0 }
$text = $text.TrimEnd("`r", "`n")

$isPassword = $false
try {
    $isPassword = [bool]$element.GetCurrentPropertyValue([System.Windows.Automation.AutomationElement]::IsPasswordProperty)
} catch {
    $isPassword = $true
}

$pidValue = 0
$appName = $null
try {
    $pidValue = [int]$element.Current.ProcessId
    if ($pidValue -gt 0) {
        $appName = (Get-Process -Id $pidValue -ErrorAction Stop).ProcessName
    }
} catch {}

$controlName = $null
try {
    $controlName = $element.Current.Name
    if ([string]::IsNullOrWhiteSpace($controlName)) { $controlName = $null }
} catch {}

[pscustomobject]@{
    text = $text
    app_name = $appName
    window_title = [AeonForegroundTitle]::Read()
    control_name = $controlName
    sensitivity = $(if ($isPassword) { "Sensitive" } else { "NonSensitive" })
} | ConvertTo-Json -Depth 4
"#;

    let Ok(output) = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output()
    else {
        return None;
    };
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    parse_text_commit_json(trimmed).ok()
}

#[cfg(not(target_os = "windows"))]
pub fn current_text_commit() -> Option<TextCommit> {
    None
}

pub async fn start_foreground_window_monitor(engine: Arc<CaptureEngine>) {
    start_foreground_window_monitor_with_interval(engine, Duration::from_millis(750)).await;
}

pub async fn start_text_commit_monitor(engine: Arc<CaptureEngine>) {
    start_text_commit_monitor_with_interval(engine, Duration::from_millis(900)).await;
}

pub async fn start_foreground_window_monitor_with_interval(
    engine: Arc<CaptureEngine>,
    tick: Duration,
) {
    let mut ticker = interval(tick);
    let mut last_signature: Option<(u32, String)> = None;

    loop {
        ticker.tick().await;
        let Some(window) = current_foreground_window() else {
            continue;
        };
        let signature = (window.pid, window.title.clone());
        if last_signature.as_ref() == Some(&signature) {
            continue;
        }
        last_signature = Some(signature);

        let activity = OsActivity::window_focus(window, OsCaptureProvider::WinEventHook);
        let Ok(entry) = activity.into_capture_entry() else {
            continue;
        };
        if let Err(err) = engine.capture(entry).await {
            tracing_like_warn(&format!("foreground window capture failed: {err}"));
        }
    }
}

pub async fn start_text_commit_monitor_with_interval(engine: Arc<CaptureEngine>, tick: Duration) {
    let mut ticker = interval(tick);
    let mut tracker = TextCommitTracker::default();

    loop {
        ticker.tick().await;
        let Some(commit) = current_text_commit() else {
            continue;
        };
        let Some(activity) = tracker.next_activity(commit, OsCaptureProvider::WindowsUiAutomation)
        else {
            continue;
        };
        let Ok(entry) = activity.into_capture_entry() else {
            continue;
        };
        if let Err(err) = engine.capture(entry).await {
            tracing_like_warn(&format!("text commit capture failed: {err}"));
        }
    }
}

fn tracing_like_warn(message: &str) {
    eprintln!("[aeon-capture] {message}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn focused_window() -> ForegroundWindow {
        ForegroundWindow {
            pid: 42,
            process_name: Some("Code".to_string()),
            title: "AEON-FLOW - Visual Studio Code".to_string(),
            bounds: Some(WindowBounds {
                left: 10,
                top: 20,
                width: 1200,
                height: 800,
            }),
        }
    }

    #[test]
    fn window_focus_entry_uses_os_activity_kind_and_provider() {
        let entry = OsActivity::window_focus(focused_window(), OsCaptureProvider::WinEventHook)
            .into_capture_entry()
            .unwrap();

        assert_eq!(entry.kind, CaptureKind::OsActivity);
        assert_eq!(
            entry.source,
            CaptureSource::OperatingSystem {
                provider: OsCaptureProvider::WinEventHook
            }
        );
        assert_eq!(
            entry.meta.title.as_deref(),
            Some("Window focus: AEON-FLOW - Visual Studio Code")
        );
        assert_eq!(entry.meta.app_name.as_deref(), Some("Code"));
        assert_eq!(
            entry.meta.extra.get("os_activity_kind").map(String::as_str),
            Some("window_focus")
        );
        assert_eq!(entry.meta.extra.get("pid").map(String::as_str), Some("42"));

        let stored: OsActivity = serde_json::from_slice(&entry.data).unwrap();
        assert_eq!(
            stored,
            OsActivity::WindowFocus {
                provider: OsCaptureProvider::WinEventHook,
                window: focused_window()
            }
        );
    }

    #[test]
    fn nonsensitive_text_commit_keeps_committed_text() {
        let entry = OsActivity::text_commit(
            TextCommit {
                text: "typed note".to_string(),
                app_name: Some("Editor".to_string()),
                window_title: Some("notes.txt".to_string()),
                control_name: Some("Document".to_string()),
                sensitivity: InputSensitivity::NonSensitive,
            },
            OsCaptureProvider::WindowsUiAutomation,
        )
        .into_capture_entry()
        .unwrap();

        let stored: OsActivity = serde_json::from_slice(&entry.data).unwrap();
        assert!(matches!(
            stored,
            OsActivity::TextCommit {
                commit: TextCommit { ref text, .. },
                ..
            } if text == "typed note"
        ));
        assert_eq!(entry.meta.summary.as_deref(), Some("typed note"));
    }

    #[test]
    fn sensitive_text_commit_is_redacted_before_storage() {
        let entry = OsActivity::text_commit(
            TextCommit {
                text: "password123".to_string(),
                app_name: Some("Browser".to_string()),
                window_title: Some("Login".to_string()),
                control_name: Some("Password".to_string()),
                sensitivity: InputSensitivity::Sensitive,
            },
            OsCaptureProvider::WindowsUiAutomation,
        )
        .into_capture_entry()
        .unwrap();

        let payload = String::from_utf8(entry.data.clone()).unwrap();
        assert!(!payload.contains("password123"));
        assert!(!entry.meta.summary.unwrap().contains("password123"));

        let stored: OsActivity = serde_json::from_slice(&entry.data).unwrap();
        assert!(matches!(
            stored,
            OsActivity::TextCommit {
                commit: TextCommit { ref text, .. },
                ..
            } if text.is_empty()
        ));
    }

    #[test]
    fn text_commit_tracker_emits_only_meaningful_changes() {
        let mut tracker = TextCommitTracker::default();
        let first = TextCommit {
            text: "draft one".to_string(),
            app_name: Some("Editor".to_string()),
            window_title: Some("notes.txt".to_string()),
            control_name: Some("Document".to_string()),
            sensitivity: InputSensitivity::NonSensitive,
        };
        let same = first.clone();
        let changed = TextCommit {
            text: "draft two".to_string(),
            ..first.clone()
        };
        let empty = TextCommit {
            text: "   ".to_string(),
            ..first.clone()
        };

        assert!(tracker
            .next_activity(first, OsCaptureProvider::WindowsUiAutomation)
            .is_some());
        assert!(tracker
            .next_activity(same, OsCaptureProvider::WindowsUiAutomation)
            .is_none());
        assert!(tracker
            .next_activity(empty, OsCaptureProvider::WindowsUiAutomation)
            .is_none());
        assert!(matches!(
            tracker.next_activity(changed, OsCaptureProvider::WindowsUiAutomation),
            Some(OsActivity::TextCommit { .. })
        ));
    }

    #[test]
    fn parses_ui_automation_text_commit_json() {
        let json = r#"{
            "text": "committed text",
            "app_name": "notepad",
            "window_title": "notes.txt - Notepad",
            "control_name": "Text Editor",
            "sensitivity": "NonSensitive"
        }"#;

        let commit = parse_text_commit_json(json).unwrap();

        assert_eq!(commit.text, "committed text");
        assert_eq!(commit.app_name.as_deref(), Some("notepad"));
        assert_eq!(commit.window_title.as_deref(), Some("notes.txt - Notepad"));
        assert_eq!(commit.control_name.as_deref(), Some("Text Editor"));
        assert_eq!(commit.sensitivity, InputSensitivity::NonSensitive);
    }
}
