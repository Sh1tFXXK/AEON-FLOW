use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// 从剪贴板读取图片，返回 PNG 字节
pub fn clipboard_image_png() -> Option<Vec<u8>> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let img = clipboard.get_image().ok()?;
    rgba_to_png(
        img.width as u32,
        img.height as u32,
        img.bytes.into_owned(),
    )
}

fn rgba_to_png(width: u32, height: u32, rgba: Vec<u8>) -> Option<Vec<u8>> {
    let img = image::RgbaImage::from_raw(width, height, rgba)?;
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    use image::ImageEncoder;
    encoder
        .write_image(img.as_raw(), width, height, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(buf)
}

/// 截图工具可能保存到的目录（含不存在路径，用于启动诊断）
pub fn screenshot_candidate_dirs() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let pictures = dirs::picture_dir().unwrap_or_else(|| home.join("Pictures"));
    let mut candidates = vec![
        pictures.join("Screenshots"),
        pictures.clone(),
    ];

    #[cfg(target_os = "linux")]
    {
        candidates.push(pictures.clone());
        candidates.push(home.join("Pictures/Screenshots"));
        candidates.push(home.clone());
        candidates.push(home.join("Pictures/Flameshot"));
        candidates.push(pictures.join("Flameshot"));
        candidates.push(pictures.join("Screenshots"));
        if let Ok(xdg) = std::env::var("XDG_PICTURES_DIR") {
            let p = PathBuf::from(xdg.replace("$HOME", &home.to_string_lossy()));
            candidates.push(p.clone());
            candidates.push(p.join("Screenshots"));
        }
        let hyprshot_conf = home.join(".config/hypr/hyprshot.conf");
        if hyprshot_conf.exists() {
            if let Ok(content) = std::fs::read_to_string(&hyprshot_conf) {
                for line in content.lines() {
                    if line.contains("output_folder") || line.contains("savedir") {
                        if let Some(path) = line.split('=').nth(1) {
                            let p =
                                PathBuf::from(path.trim().replace('~', &home.to_string_lossy()));
                            candidates.push(p);
                        }
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        candidates.push(pictures.join("Screenshots"));
        candidates.push(pictures.clone());
        if let Ok(user) = std::env::var("USERPROFILE") {
            let user = PathBuf::from(user);
            candidates.push(user.join("OneDrive/Pictures/Screenshots"));
            candidates.push(user.join("Pictures/Screenshots"));
        }
        candidates.push(home.join("Documents/WeChat Files"));
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push(dirs::desktop_dir().unwrap_or_else(|| home.join("Desktop")));
        candidates.push(pictures.clone());
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "com.apple.screencapture", "location"])
            .output()
        {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if !path.is_empty() {
                    candidates.push(PathBuf::from(path));
                }
            }
        }
    }

    let mut seen = HashSet::new();
    candidates.into_iter().filter(|p| seen.insert(p.clone())).collect()
}

/// 已存在、可监听的截图保存目录
pub fn screenshot_save_dirs() -> Vec<PathBuf> {
    screenshot_candidate_dirs()
        .into_iter()
        .filter(|d| d.exists())
        .collect()
}

/// 通用图片目录监控（文件监控模块用）
pub fn image_watch_dirs() -> Vec<PathBuf> {
    screenshot_save_dirs()
}

/// 截图文件扩展名判断
pub fn is_screenshot_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp")
    )
}

/// 判断文件是否是图片
pub fn is_image_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => matches!(
            ext.to_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tiff" | "tif" | "heic" | "heif"
        ),
        None => false,
    }
}

/// 读取图片文件，统一转为 PNG 字节
pub fn read_image_as_png(path: &Path) -> Option<Vec<u8>> {
    std::thread::sleep(std::time::Duration::from_millis(200));
    let data = std::fs::read(path).ok()?;
    if data.starts_with(b"\x89PNG") {
        return Some(data);
    }
    let img = image::load_from_memory(&data).ok()?;
    let mut buf = Vec::new();
    img.write_to(
        &mut std::io::Cursor::new(&mut buf),
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(buf)
}

/// 获取图片尺寸
pub fn image_dimensions(png: &[u8]) -> (u32, u32) {
    image::load_from_memory(png)
        .map(|img| (img.width(), img.height()))
        .unwrap_or((0, 0))
}
