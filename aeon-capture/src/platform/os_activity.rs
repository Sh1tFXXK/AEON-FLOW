use crate::capture::OsCaptureProvider;
use crate::os_activity::{ForegroundWindow, InputSensitivity, TextCommit};

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
    $bounds = [pscustomobject]@{left=$rect.Left;top=$rect.Top;width=$rect.Right-$rect.Left;height=$rect.Bottom-$rect.Top}
}
$processName = $null
try { $processName = (Get-Process -Id $pidValue -ErrorAction Stop).ProcessName } catch {}
[pscustomobject]@{pid=[uint32]$pidValue;process_name=$processName;title=$title;bounds=$bounds} | ConvertTo-Json -Depth 4
"#;
    let output = std::process::Command::new("powershell.exe").args(["-NoProfile","-ExecutionPolicy","Bypass","-Command",script]).output().ok()?;
    if !output.status.success() { return None; }
    let trimmed = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if trimmed.is_empty() { return None; }
    serde_json::from_str::<ForegroundWindow>(&trimmed).ok()
}

#[cfg(target_os = "linux")]
pub fn current_foreground_window() -> Option<ForegroundWindow> {
    if let Ok(output) = std::process::Command::new("hyprctl").args(["activewindow", "-j"]).output() {
        if output.status.success() {
            let v: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
            let title = v.get("title")?.as_str()?.trim().to_string();
            if !title.is_empty() {
                let pid = v.get("pid").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
                let process_name = v.get("class").and_then(|x| x.as_str()).map(|s| s.to_string());
                return Some(ForegroundWindow { pid, process_name, title, bounds: None });
            }
        }
    }
    let win = std::process::Command::new("xdotool").args(["getactivewindow"]).output().ok()?;
    if !win.status.success() { return None; }
    let win_id = String::from_utf8(win.stdout).ok()?.trim().to_string();
    if win_id.is_empty() { return None; }
    let title_out = std::process::Command::new("xdotool").args(["getwindowname", &win_id]).output().ok()?;
    let pid_out = std::process::Command::new("xdotool").args(["getwindowpid", &win_id]).output().ok()?;
    let title = String::from_utf8(title_out.stdout).ok()?.trim().to_string();
    if title.is_empty() { return None; }
    let pid = String::from_utf8(pid_out.stdout).ok()?.trim().parse::<u32>().unwrap_or(0);
    let process_name = if pid > 0 { std::fs::read_to_string(format!("/proc/{pid}/comm")).ok().map(|s| s.trim().to_string()) } else { None };
    Some(ForegroundWindow { pid, process_name, title, bounds: None })
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub fn current_foreground_window() -> Option<ForegroundWindow> { None }

#[cfg(target_os = "windows")]
pub fn current_text_commit() -> Option<TextCommit> { None }

#[cfg(target_os = "linux")]
pub fn current_text_commit() -> Option<TextCommit> {
    let text = if let Ok(out) = std::process::Command::new("wl-paste").args(["-n"]).output() {
        if out.status.success() { String::from_utf8(out.stdout).ok() } else { None }
    } else { None }
    .or_else(|| {
        let out = std::process::Command::new("xclip").args(["-selection", "clipboard", "-o"]).output().ok()?;
        if !out.status.success() { return None; }
        String::from_utf8(out.stdout).ok()
    })?;
    let trimmed = text.trim().to_string(); if trimmed.is_empty() { return None; }
    let window = current_foreground_window();
    Some(TextCommit { text: trimmed, app_name: window.as_ref().and_then(|w| w.process_name.clone()), window_title: window.as_ref().map(|w| w.title.clone()), control_name: None, sensitivity: InputSensitivity::NonSensitive })
}

#[cfg(all(not(target_os = "windows"), not(target_os = "linux")))]
pub fn current_text_commit() -> Option<TextCommit> { None }

pub fn text_commit_provider() -> OsCaptureProvider {
    #[cfg(target_os = "windows")]
    { return OsCaptureProvider::WindowsUiAutomation; }
    #[cfg(not(target_os = "windows"))]
    { return OsCaptureProvider::ShellNotification; }
}

pub fn foreground_provider() -> OsCaptureProvider {
    #[cfg(target_os = "windows")]
    { return OsCaptureProvider::WinEventHook; }
    #[cfg(not(target_os = "windows"))]
    { return OsCaptureProvider::ShellNotification; }
}
