use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use std::sync::Arc;
use tokio::time::{interval, Duration};

#[cfg(target_os = "windows")]
pub async fn start_clipboard_monitor(engine: Arc<CaptureEngine>) {
    use clipboard_win::{formats, get_clipboard};

    let mut last_cid: Option<[u8; 32]> = None;
    let mut ticker = interval(Duration::from_millis(500));

    loop {
        ticker.tick().await;

        let text: Result<String, _> = get_clipboard(formats::Unicode);
        let Ok(text) = text else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }

        let data = text.as_bytes().to_vec();
        let cid = *blake3::hash(&data).as_bytes();
        if Some(cid) == last_cid {
            continue;
        }
        last_cid = Some(cid);

        let kind = detect_text_kind(&text);
        let entry = CaptureEntry::new(data, kind, CaptureSource::Clipboard);
        if let Err(err) = engine.capture(entry).await {
            tracing_like_warn(&format!("clipboard capture failed: {err}"));
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub async fn start_clipboard_monitor(_engine: Arc<CaptureEngine>) {
    futures_pending().await;
}

pub fn detect_text_kind(text: &str) -> CaptureKind {
    let trimmed = text.trim();

    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return CaptureKind::Webpage;
    }

    let code_indicators = [
        "{", "fn ", "def ", "class ", "import ", "const ", "let ", "var ",
    ];
    let line_count = text.lines().count();
    let has_code = code_indicators
        .iter()
        .any(|indicator| text.contains(indicator));

    if has_code && line_count > 2 {
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
    if code.contains("def ") && code.contains("import ") {
        return "Python".to_string();
    }
    if code.contains("function") || code.contains("const ") {
        return "JavaScript".to_string();
    }
    if code.contains("class ") && code.contains("public ") {
        return "Java".to_string();
    }
    "代码".to_string()
}

#[cfg(not(target_os = "windows"))]
async fn futures_pending() {
    std::future::pending::<()>().await;
}

fn tracing_like_warn(message: &str) {
    eprintln!("[aeon-capture] {message}");
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
