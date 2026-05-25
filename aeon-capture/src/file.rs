use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::clipboard::detect_text_kind;
use crate::engine::CaptureEngine;
use crate::platform::image::{image_dimensions as platform_image_dimensions, is_image_file, read_image_as_png};
use crate::screenshot::{image_dimensions, is_image};
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub fn start_file_monitor(
    engine: Arc<CaptureEngine>,
    roots: Vec<PathBuf>,
) -> notify::Result<RecommendedWatcher> {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;

    for root in roots {
        if root.exists() {
            watcher.watch(&root, RecursiveMode::Recursive)?;
        }
    }

    let handle = tokio::runtime::Handle::current();
    std::thread::spawn(move || {
        for event in rx.into_iter().flatten() {
            for path in event.paths {
                if should_capture_path(&path) {
                    let engine = engine.clone();
                    handle.spawn(async move {
                        capture_path(
                            engine,
                            path,
                            CaptureSource::FileWatch {
                                path: String::new(),
                            },
                        )
                        .await;
                    });
                }
            }
        }
    });

    Ok(watcher)
}

pub async fn capture_path(
    engine: Arc<CaptureEngine>,
    path: PathBuf,
    mut source: CaptureSource,
) -> Option<crate::capture::CID> {
    tokio::time::sleep(tokio::time::Duration::from_millis(180)).await;

    let metadata = tokio::fs::metadata(&path).await.ok()?;
    if !metadata.is_file() {
        return None;
    }
    if let CaptureSource::FileWatch { path: source_path } = &mut source {
        if source_path.is_empty() {
            *source_path = path.to_string_lossy().to_string();
        }
    }

    if is_image_file(&path) {
        let path_for_blocking = path.clone();
        let png_result = tokio::task::spawn_blocking(move || read_image_as_png(&path_for_blocking))
            .await
            .ok()
            .flatten();
        if let Some(png) = png_result {
            let (width, height) = platform_image_dimensions(&png);
            let title = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("captured-image")
                .to_string();
            let mut entry = CaptureEntry::new(
                png,
                CaptureKind::Image {
                    width,
                    height,
                    format: "png".into(),
                },
                source,
            )
            .with_title(&title);
            entry.meta.file_path = Some(path.to_string_lossy().to_string());
            return engine.capture(entry).await.ok();
        }
    }

    let data = tokio::fs::read(&path).await.ok()?;
    if data.is_empty() {
        return None;
    }

    let kind = kind_from_path(&path, &data);
    let title = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("captured-file")
        .to_string();
    let mut entry = CaptureEntry::new(data, kind, source).with_title(&title);
    entry.meta.file_path = Some(path.to_string_lossy().to_string());

    engine.capture(entry).await.ok()
}

pub fn kind_from_path(path: &Path, data: &[u8]) -> CaptureKind {
    if is_image(path) {
        let format = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let (width, height) = image_dimensions(data).unwrap_or((0, 0));
        return CaptureKind::Image {
            width,
            height,
            format,
        };
    }

    let lower = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    if let Some(language) = language_from_filename(&lower) {
        return CaptureKind::Code {
            language: language.to_string(),
        };
    }

    if lower.ends_with(".pdf") {
        return CaptureKind::Document {
            format: "pdf".to_string(),
        };
    }
    if lower.ends_with(".doc") || lower.ends_with(".docx") {
        return CaptureKind::Document {
            format: "word".to_string(),
        };
    }

    if let Ok(text) = std::str::from_utf8(data) {
        return detect_text_kind(text);
    }

    CaptureKind::Blob {
        mime: aeon_store::mime_from_path(path),
    }
}

fn should_capture_path(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    let path_text = path.to_string_lossy();
    !path_text.contains(".aeon-history")
        && !path_text.contains(".aeon-meta")
        && !path_text.ends_with(".tmp")
        && !path_text.ends_with(".part")
}

fn language_from_filename(name: &str) -> Option<&'static str> {
    if name.ends_with(".rs") {
        Some("Rust")
    } else if name.ends_with(".py") {
        Some("Python")
    } else if name.ends_with(".js")
        || name.ends_with(".ts")
        || name.ends_with(".jsx")
        || name.ends_with(".tsx")
    {
        Some("JavaScript")
    } else if name.ends_with(".java") || name.ends_with(".kt") {
        Some("Java")
    } else if name.ends_with(".go") {
        Some("Go")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_code_by_extension() {
        assert_eq!(
            kind_from_path(Path::new("main.rs"), b"fn main() {}"),
            CaptureKind::Code {
                language: "Rust".to_string()
            }
        );
    }

    #[test]
    fn classifies_utf8_file_by_content() {
        assert_eq!(
            kind_from_path(Path::new("clip.txt"), b"https://example.com"),
            CaptureKind::Webpage
        );
    }
}
