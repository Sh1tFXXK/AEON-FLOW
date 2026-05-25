use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use crate::platform::clipboard_read;
use crate::platform::image::image_dimensions;
use std::sync::Arc;
use tokio::time::{interval, Duration};

pub async fn start_clipboard_monitor(engine: Arc<CaptureEngine>) {
    let probe = tokio::task::spawn_blocking(clipboard_read::probe)
        .await
        .unwrap_or(Err("spawn failed".into()));

    match probe {
        Ok(backend) => println!("[capture/clipboard] OK (backend: {backend})"),
        Err(e) => {
            eprintln!("[capture/clipboard] FAILED: {e}");
            eprintln!("[capture/clipboard] Arch/Wayland: sudo pacman -S wl-clipboard");
            eprintln!("[capture/clipboard] Arch/X11:     sudo pacman -S xclip");
            return;
        }
    }

    let mut text_hash: u64 = 0;
    let mut image_hash: u64 = 0;
    let mut ticker = interval(Duration::from_millis(500));

    loop {
        ticker.tick().await;

        let sample = tokio::task::spawn_blocking(|| {
            (
                clipboard_read::read_text(),
                clipboard_read::read_image_png(),
            )
        })
        .await
        .unwrap_or((None, None));

        if let Some(text) = sample.0 {
            let h = quick_hash(text.as_bytes());
            if h != text_hash {
                text_hash = h;
                let kind = detect_text_kind(&text);
                let entry = CaptureEntry::new(text.into_bytes(), kind, CaptureSource::Clipboard);
                if let Err(e) = engine.capture(entry).await {
                    eprintln!("[capture/clipboard] text capture failed: {e}");
                }
            }
        }

        if let Some(png) = sample.1 {
            let h = quick_hash(&png[..png.len().min(512)]);
            if h != image_hash {
                image_hash = h;
                let (w, h_px) = image_dimensions(&png);
                let entry = CaptureEntry::new(
                    png,
                    CaptureKind::Image {
                        width: w,
                        height: h_px,
                        format: "png".into(),
                    },
                    CaptureSource::Clipboard,
                )
                .with_title("clipboard-image");
                if let Err(e) = engine.capture(entry).await {
                    eprintln!("[capture/clipboard] image capture failed: {e}");
                } else {
                    println!("[capture/clipboard] image captured {w}x{h_px}");
                }
            }
        }
    }
}

fn quick_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut h);
    h.finish()
}

pub fn detect_text_kind(text: &str) -> CaptureKind {
    let trimmed = text.trim();

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return CaptureKind::Webpage;
    }

    let code_indicators = [
        "{", "fn ", "def ", "class ", "import ", "const ", "let ", "var ", "func ",
    ];
    let line_count = text.lines().count();
    let has_code = code_indicators.iter().any(|indicator| text.contains(indicator));

    if has_code && line_count > 3 {
        return CaptureKind::Code {
            language: detect_language(text),
        };
    }

    CaptureKind::Clipboard
}

pub fn detect_language(code: &str) -> String {
    if code.contains("fn ") && code.contains("let ") {
        return "Rust".to_string();
    }
    if code.contains("def ") || code.contains("import ") {
        return "Python".to_string();
    }
    if code.contains("function") || code.contains("const ") {
        return "JavaScript".to_string();
    }
    if code.contains("class ") && code.contains("public ") {
        return "Java".to_string();
    }
    "code".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_urls_as_webpages() {
        assert_eq!(
            detect_text_kind("https://example.com"),
            CaptureKind::Webpage
        );
    }

    #[test]
    fn detects_rust_snippets() {
        let kind = detect_text_kind("fn main() {\n  let x = 1;\n  println!(\"{x}\");\n}");
        assert_eq!(
            kind,
            CaptureKind::Code {
                language: "Rust".to_string()
            }
        );
    }
}
