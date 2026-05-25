use std::process::Command;

/// 启动探测：Wayland 优先 wl-paste，否则 arboard / xclip
pub fn probe() -> Result<&'static str, String> {
    #[cfg(target_os = "linux")]
    {
        if std::env::var("WAYLAND_DISPLAY").is_ok()
            && Command::new("wl-paste")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        {
            return Ok("wl-paste");
        }
        if Command::new("xclip")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return Ok("xclip");
        }
    }
    if arboard::Clipboard::new().is_ok() {
        return Ok("arboard");
    }
    Err("no clipboard backend (install wl-clipboard or xclip)".into())
}

pub fn read_text() -> Option<String> {
    #[cfg(target_os = "linux")]
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        if let Some(text) = linux_read_text() {
            return Some(text);
        }
    }

    if let Ok(mut cb) = arboard::Clipboard::new() {
        if let Ok(text) = cb.get_text() {
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        return linux_read_text();
    }

    #[cfg(not(target_os = "linux"))]
    None
}

pub fn read_image_png() -> Option<Vec<u8>> {
    #[cfg(target_os = "linux")]
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        if let Some(png) = linux_read_image_png() {
            return Some(png);
        }
    }

    super::image::clipboard_image_png().or_else(|| {
        #[cfg(target_os = "linux")]
        {
            linux_read_image_png()
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    })
}

#[cfg(target_os = "linux")]
fn linux_read_text() -> Option<String> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        if let Ok(out) = Command::new("wl-paste").args(["-n"]).output() {
            if out.status.success() {
                if let Ok(text) = String::from_utf8(out.stdout) {
                    if !text.trim().is_empty() {
                        return Some(text);
                    }
                }
            }
        }
    }
    if let Ok(out) = Command::new("xclip")
        .args(["-selection", "clipboard", "-o"])
        .output()
    {
        if out.status.success() {
            if let Ok(text) = String::from_utf8(out.stdout) {
                if !text.trim().is_empty() {
                    return Some(text);
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn linux_read_image_png() -> Option<Vec<u8>> {
    if std::env::var("WAYLAND_DISPLAY").is_ok() {
        for mime in ["image/png", "image/jpeg", "image/webp", "image/bmp"] {
            if let Some(png) = wl_paste_image(mime) {
                return Some(png);
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn wl_paste_image(mime: &str) -> Option<Vec<u8>> {
    let out = Command::new("wl-paste")
        .args(["-t", mime])
        .output()
        .ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    bytes_to_png(&out.stdout)
}

#[cfg(target_os = "linux")]
fn bytes_to_png(data: &[u8]) -> Option<Vec<u8>> {
    if data.starts_with(b"\x89PNG") {
        return Some(data.to_vec());
    }
    let img = image::load_from_memory(data).ok()?;
    let mut buf = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(buf)
}
