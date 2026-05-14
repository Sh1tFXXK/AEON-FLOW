use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn start_screenshot_monitor(engine: Arc<CaptureEngine>) -> notify::Result<RecommendedWatcher> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

    for dir in screenshot_dirs() {
        if dir.exists() {
            watcher.watch(&dir, RecursiveMode::Recursive)?;
        }
    }

    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        for event in rx.into_iter().flatten() {
            for path in event.paths {
                if is_image(&path) {
                    let engine = engine.clone();
                    handle.spawn(async move {
                        capture_image(engine, path).await;
                    });
                }
            }
        }
    });

    Ok(watcher)
}

pub fn screenshot_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(picture_dir) = dirs::picture_dir() {
        dirs.push(picture_dir.join("Screenshots"));
        dirs.push(picture_dir.join("屏幕截图"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join("Documents").join("WeChat Files"));
        dirs.push(home.join("Documents").join("Tencent Files"));
    }
    dirs
}

pub async fn capture_image(engine: Arc<CaptureEngine>, path: PathBuf) {
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let Ok(data) = tokio::fs::read(&path).await else {
        return;
    };
    let (width, height) = image_dimensions(&data).unwrap_or((0, 0));
    let format = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("png")
        .to_lowercase();
    let title = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("screenshot")
        .to_string();

    let mut entry = CaptureEntry::new(
        data,
        CaptureKind::Image {
            width,
            height,
            format,
        },
        CaptureSource::Screenshot,
    )
    .with_title(&title);
    entry.meta.file_path = Some(path.to_string_lossy().to_string());

    let _ = engine.capture(entry).await;
}

#[cfg(target_os = "windows")]
pub fn capture_window_screenshot_bytes(pid: u32) -> Result<(Vec<u8>, u32, u32), String> {
    let out = std::env::temp_dir().join(format!(
        "aeon-window-{}-{}.png",
        pid,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    let out_literal = out.display().to_string().replace('\'', "''");
    let script = format!(
        r#"
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class AeonWindowCapture {{
    public delegate bool EnumWindowsProc(IntPtr hWnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc proc, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hWnd, out RECT rect);
    public struct RECT {{ public int Left; public int Top; public int Right; public int Bottom; }}
    public static IntPtr FindMainWindow(uint targetPid) {{
        IntPtr found = IntPtr.Zero;
        EnumWindows((hWnd, lParam) => {{
            uint windowPid;
            GetWindowThreadProcessId(hWnd, out windowPid);
            if (windowPid != targetPid || !IsWindowVisible(hWnd)) return true;
            RECT rect;
            if (!GetWindowRect(hWnd, out rect)) return true;
            if (rect.Right <= rect.Left || rect.Bottom <= rect.Top) return true;
            found = hWnd;
            return false;
        }}, IntPtr.Zero);
        return found;
    }}
}}
"@
$pidValue = {pid}
$out = '{out_literal}'
$hwnd = [AeonWindowCapture]::FindMainWindow([uint32]$pidValue)
if ($hwnd -eq [IntPtr]::Zero) {{ throw "No visible main window for PID $pidValue" }}
$rect = New-Object AeonWindowCapture+RECT
if (-not [AeonWindowCapture]::GetWindowRect($hwnd, [ref]$rect)) {{ throw "Cannot read window bounds" }}
$width = $rect.Right - $rect.Left
$height = $rect.Bottom - $rect.Top
if ($width -le 0 -or $height -le 0) {{ throw "Window has empty bounds" }}
$bitmap = New-Object System.Drawing.Bitmap $width, $height
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {{
    $graphics.CopyFromScreen($rect.Left, $rect.Top, 0, 0, $bitmap.Size)
    $bitmap.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
}} finally {{
    $graphics.Dispose()
    $bitmap.Dispose()
}}
"#
    );

    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .output()
        .map_err(|err| format!("start PowerShell screenshot helper: {err}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let _ = std::fs::remove_file(&out);
        return Err(format!("window screenshot failed: {stderr}{stdout}"));
    }

    let data =
        std::fs::read(&out).map_err(|err| format!("read screenshot {}: {}", out.display(), err))?;
    let _ = std::fs::remove_file(&out);
    let (width, height) = image_dimensions(&data).unwrap_or((0, 0));
    Ok((data, width, height))
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window_screenshot_bytes(_pid: u32) -> Result<(Vec<u8>, u32, u32), String> {
    Err("window screenshot capture is currently implemented on Windows only".to_string())
}

pub fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp")
    )
}

pub fn image_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() >= 24 && data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((
            u32::from_be_bytes(data[16..20].try_into().ok()?),
            u32::from_be_bytes(data[20..24].try_into().ok()?),
        ));
    }

    if data.len() >= 10 && data.starts_with(b"GIF") {
        return Some((
            u16::from_le_bytes(data[6..8].try_into().ok()?) as u32,
            u16::from_le_bytes(data[8..10].try_into().ok()?) as u32,
        ));
    }

    if data.len() >= 26 && data.starts_with(b"BM") {
        return Some((
            i32::from_le_bytes(data[18..22].try_into().ok()?) as u32,
            i32::from_le_bytes(data[22..26].try_into().ok()?).unsigned_abs(),
        ));
    }

    jpeg_dimensions(data)
}

fn jpeg_dimensions(data: &[u8]) -> Option<(u32, u32)> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }

    let mut i = 2usize;
    while i + 9 < data.len() {
        if data[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        i += 2;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if i + 2 > data.len() {
            break;
        }
        let len = u16::from_be_bytes(data[i..i + 2].try_into().ok()?) as usize;
        if len < 2 || i + len > data.len() {
            break;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if len < 7 {
                return None;
            }
            let height = u16::from_be_bytes(data[i + 3..i + 5].try_into().ok()?) as u32;
            let width = u16::from_be_bytes(data[i + 5..i + 7].try_into().ok()?) as u32;
            return Some((width, height));
        }
        i += len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_image_extensions_case_insensitively() {
        assert!(is_image(Path::new("shot.PNG")));
        assert!(!is_image(Path::new("note.txt")));
    }

    #[test]
    fn reads_png_dimensions() {
        let mut data = b"\x89PNG\r\n\x1a\n".to_vec();
        data.extend_from_slice(&[0; 8]);
        data.extend_from_slice(&1920u32.to_be_bytes());
        data.extend_from_slice(&1080u32.to_be_bytes());

        assert_eq!(image_dimensions(&data), Some((1920, 1080)));
    }
}
