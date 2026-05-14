use super::util::process_exists;
use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct BrowserCapture {
    pub browser: String,
}

impl AppCapture for BrowserCapture {
    fn app_name(&self) -> &str {
        &self.browser
    }

    fn is_running(&self) -> bool {
        let exe = if self.browser == "Chrome" {
            "chrome.exe"
        } else {
            "firefox.exe"
        };
        process_exists(exe)
    }

    fn capture(&self) -> Option<CaptureEntry> {
        let tab = if self.browser == "Chrome" {
            latest_chrome_page()?
        } else {
            latest_firefox_page()?
        };
        let data = serde_json::to_vec(&serde_json::json!({
            "url": tab.url,
            "title": tab.title,
            "captured_at": super::util::now_ms(),
            "capture_mode": "latest-history-entry"
        }))
        .ok()?;

        let mut entry = CaptureEntry::new(
            data,
            CaptureKind::Webpage,
            CaptureSource::AppApi {
                app: self.browser.clone(),
            },
        )
        .with_title(&tab.title);
        entry.meta.url = Some(tab.url);
        entry.meta.app_name = Some(self.browser.clone());
        entry.meta.extra.insert(
            "capture_mode".to_string(),
            "latest-history-entry".to_string(),
        );
        Some(entry)
    }

    fn watch(&self, _engine: Arc<CaptureEngine>) {}
}

struct BrowserPage {
    title: String,
    url: String,
}

fn latest_chrome_page() -> Option<BrowserPage> {
    let history = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)?
        .join("Google")
        .join("Chrome")
        .join("User Data")
        .join("Default")
        .join("History");
    query_latest_history(
        &history,
        "select title, url from urls order by last_visit_time desc limit 1",
    )
}

fn latest_firefox_page() -> Option<BrowserPage> {
    let profiles = dirs::data_dir()?
        .join("Mozilla")
        .join("Firefox")
        .join("Profiles");
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for profile in std::fs::read_dir(profiles).ok()?.flatten() {
        let path = profile.path().join("places.sqlite");
        if !path.exists() {
            continue;
        }
        let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
        if newest
            .as_ref()
            .is_none_or(|(current, _)| modified > *current)
        {
            newest = Some((modified, path));
        }
    }
    let (_, history) = newest?;
    query_latest_history(
        &history,
        "select title, url from moz_places where url is not null order by last_visit_date desc limit 1",
    )
}

fn query_latest_history(path: &Path, sql: &str) -> Option<BrowserPage> {
    let temp = std::env::temp_dir().join(format!(
        "aeon-browser-history-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos()
    ));
    std::fs::copy(path, &temp).ok()?;
    let result = (|| {
        let conn = Connection::open(&temp).ok()?;
        let mut stmt = conn.prepare(sql).ok()?;
        stmt.query_row([], |row| {
            Ok(BrowserPage {
                title: row
                    .get::<_, Option<String>>(0)?
                    .unwrap_or_else(|| "网页".to_string()),
                url: row.get(1)?,
            })
        })
        .ok()
    })();
    let _ = std::fs::remove_file(temp);
    result
}
