use super::entries::{CapturePayload, EventPayload};
use super::*;

pub(super) fn capture_payload(record: CaptureRecord) -> CapturePayload {
    let cid = hex_cid(&record.cid);
    CapturePayload {
        raw_url: format!("/api/entry/{cid}/raw"),
        cid,
        kind: record.kind.key().to_string(),
        kind_label: kind_label(&record.kind).to_string(),
        title: record
            .meta
            .title
            .clone()
            .unwrap_or_else(|| fallback_title(&record)),
        summary: record.meta.summary.clone(),
        source: source_key(&record.source).to_string(),
        source_label: source_label(&record.source),
        captured_at: record.captured_at,
        size: record.size,
        mime: record.mime,
        app_name: record.meta.app_name.clone(),
        file_path: record.meta.file_path.clone(),
        url: record.meta.url.clone(),
        message_count: record.meta.message_count,
        previous_version: record.meta.previous_version.map(|cid| hex_cid(&cid)),
        extra: record.meta.extra.clone(),
        editable: is_editable_kind(&record.kind),
    }
}

pub(super) fn event_payload(event: AeonEvent) -> EventPayload {
    EventPayload {
        id: event.id.to_hex(),
        ts: event.ts,
        kind: event.kind,
        source: event.source,
        device: hex_bytes_local(&event.device),
        identity: hex_bytes_local(&event.identity),
    }
}

pub(super) fn hex_bytes_local(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(super) fn capture_record_from_entry(entry: &CaptureEntry) -> CaptureRecord {
    CaptureRecord {
        cid: entry.cid,
        kind: entry.kind.clone(),
        meta: entry.meta.clone(),
        source: entry.source.clone(),
        captured_at: entry.captured_at,
        by: entry.by,
        device: entry.device,
        size: entry.data.len(),
        mime: entry.mime(),
    }
}

pub(super) fn fallback_title(record: &CaptureRecord) -> String {
    match &record.kind {
        CaptureKind::Conversation => {
            format!("Conversation ({})", record.meta.message_count.unwrap_or(0))
        }
        CaptureKind::Code { language } => format!("{language} code"),
        CaptureKind::Image { width, height, .. } => format!("Image {width}x{height}"),
        CaptureKind::Webpage => record
            .meta
            .url
            .clone()
            .unwrap_or_else(|| "Webpage".to_string()),
        CaptureKind::Clipboard => "Clipboard".to_string(),
        CaptureKind::Text => "Text".to_string(),
        CaptureKind::Document { format } => format!("{format} document"),
        CaptureKind::ProcessState => "Process state".to_string(),
        CaptureKind::OsActivity => "OS activity".to_string(),
        CaptureKind::VmSnapshot => "AEON VM snapshot".to_string(),
        CaptureKind::Blob { .. } => "Binary content".to_string(),
    }
}

pub(super) fn kind_label(kind: &CaptureKind) -> &'static str {
    match kind {
        CaptureKind::Conversation => "Conversation",
        CaptureKind::Code { .. } => "Code",
        CaptureKind::Text => "Text",
        CaptureKind::Image { .. } => "Image",
        CaptureKind::Webpage => "Webpage",
        CaptureKind::Document { .. } => "Document",
        CaptureKind::ProcessState => "Process",
        CaptureKind::OsActivity => "OS activity",
        CaptureKind::VmSnapshot => "VM snapshot",
        CaptureKind::Clipboard => "Clipboard",
        CaptureKind::Blob { .. } => "File",
    }
}

pub(super) fn is_editable_kind(kind: &CaptureKind) -> bool {
    matches!(
        kind,
        CaptureKind::Conversation
            | CaptureKind::Code { .. }
            | CaptureKind::Text
            | CaptureKind::Webpage
            | CaptureKind::ProcessState
            | CaptureKind::Clipboard
    )
}

pub(super) fn source_key(source: &CaptureSource) -> &'static str {
    match source {
        CaptureSource::DragDrop => "DragDrop",
        CaptureSource::Clipboard => "Clipboard",
        CaptureSource::Screenshot => "Screenshot",
        CaptureSource::FileWatch { .. } => "FileWatch",
        CaptureSource::AppApi { .. } => "AppApi",
        CaptureSource::OperatingSystem { .. } => "OperatingSystem",
        CaptureSource::ShareMenu => "ShareMenu",
        CaptureSource::Manual => "Manual",
        CaptureSource::PeerSync { .. } => "PeerSync",
    }
}

pub(super) fn source_label(source: &CaptureSource) -> String {
    match source {
        CaptureSource::DragDrop => "Drag drop".to_string(),
        CaptureSource::Clipboard => "Clipboard".to_string(),
        CaptureSource::Screenshot => "Screenshot".to_string(),
        CaptureSource::FileWatch { path } => format!("File watch: {path}"),
        CaptureSource::AppApi { app } => app.clone(),
        CaptureSource::OperatingSystem { provider } => format!("OS: {provider:?}"),
        CaptureSource::ShareMenu => "Share menu".to_string(),
        CaptureSource::Manual => "Manual".to_string(),
        CaptureSource::PeerSync { device_name } => format!("Peer sync: {device_name}"),
    }
}

pub(super) fn source_from_peer_headers(
    headers: &HeaderMap,
    default: CaptureSource,
) -> CaptureSource {
    match header_value(headers, "x-aeon-device-kind")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "android" => CaptureSource::ShareMenu,
        "" => default,
        _ => header_value(headers, "x-aeon-device-name")
            .map(|device_name| CaptureSource::PeerSync { device_name })
            .unwrap_or(default),
    }
}

pub(super) fn annotate_peer_metadata(entry: &mut CaptureEntry, headers: &HeaderMap) {
    if let Some(device_id) = header_value(headers, "x-aeon-device-id") {
        entry
            .meta
            .extra
            .insert("source_device_id".to_string(), device_id);
    }
    if let Some(device_name) = header_value(headers, "x-aeon-device-name") {
        entry
            .meta
            .extra
            .insert("source_device_name".to_string(), device_name.clone());
        if entry.meta.app_name.is_none() {
            entry.meta.app_name = Some(device_name);
        }
    }
    if let Some(device_kind) = header_value(headers, "x-aeon-device-kind") {
        entry
            .meta
            .extra
            .insert("source_device_kind".to_string(), device_kind.clone());
        if entry.meta.app_name.is_none() && device_kind.eq_ignore_ascii_case("android") {
            entry.meta.app_name = Some("Android".to_string());
        }
    }
}

pub(super) fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(super) fn is_http_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub(super) fn stamp_capture_identity(entry: &mut CaptureEntry, state: &AppState) {
    entry.by = state.identity_id;
    entry.device = state.device_id;
}

pub(super) fn capture_kind_for_file(name: &str, mime: &str, data: &[u8]) -> CaptureKind {
    let lower = name.to_ascii_lowercase();
    if mime.starts_with("image/") {
        let format = lower
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_string())
            .unwrap_or_else(|| mime.trim_start_matches("image/").to_string());
        let (width, height) = aeon_capture::screenshot::image_dimensions(data).unwrap_or((0, 0));
        return CaptureKind::Image {
            width,
            height,
            format,
        };
    }

    if let Some(language) = language_from_filename(&lower) {
        return CaptureKind::Code {
            language: language.to_string(),
        };
    }

    if mime.starts_with("text/") {
        if let Ok(text) = std::str::from_utf8(data) {
            return aeon_capture::clipboard::detect_text_kind(text);
        }
        return CaptureKind::Text;
    }

    if lower.ends_with(".pdf") {
        return CaptureKind::Document {
            format: "pdf".to_string(),
        };
    }
    if lower.ends_with(".docx") || lower.ends_with(".doc") {
        return CaptureKind::Document {
            format: "word".to_string(),
        };
    }

    CaptureKind::Blob {
        mime: mime.to_string(),
    }
}

pub(super) fn language_from_filename(name: &str) -> Option<&'static str> {
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
    } else if name.ends_with(".java") {
        Some("Java")
    } else if name.ends_with(".go") {
        Some("Go")
    } else {
        None
    }
}

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
