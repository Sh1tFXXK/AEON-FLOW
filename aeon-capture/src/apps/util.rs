use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[cfg(windows)]
pub fn process_exists(exe_name: &str) -> bool {
    let Ok(output) = std::process::Command::new("tasklist")
        .args([
            "/FI",
            &format!("IMAGENAME eq {exe_name}"),
            "/FO",
            "CSV",
            "/NH",
        ])
        .output()
    else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    stdout.contains(&exe_name.to_ascii_lowercase())
}

#[cfg(not(windows))]
pub fn process_exists(_exe_name: &str) -> bool {
    false
}

pub fn find_latest_file(root: &Path, extension: &str) -> Option<PathBuf> {
    let mut latest: Option<(SystemTime, PathBuf)> = None;
    visit_files(root, &mut |path| {
        let ext_matches = path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension));
        if !ext_matches {
            return;
        }
        let Ok(meta) = std::fs::metadata(path) else {
            return;
        };
        let Ok(modified) = meta.modified() else {
            return;
        };
        if latest
            .as_ref()
            .is_none_or(|(current, _)| modified > *current)
        {
            latest = Some((modified, path.to_path_buf()));
        }
    });
    latest.map(|(_, path)| path)
}

pub fn visit_files(root: &Path, f: &mut impl FnMut(&Path)) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            visit_files(&path, f);
        } else if file_type.is_file() {
            f(&path);
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
