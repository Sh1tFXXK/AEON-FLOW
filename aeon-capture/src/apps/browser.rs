use super::util::process_exists;
use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use scraper::{Html, Selector};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

const MAX_WEBPAGE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_WEBPAGE_TEXT_CHARS: usize = 80_000;

pub struct BrowserCapture {
    pub browser: String,
}

impl AppCapture for BrowserCapture {
    fn app_name(&self) -> &str {
        &self.browser
    }

    fn is_running(&self) -> bool {
        let exe = match self.browser.as_str() {
            "Chrome" => "chrome.exe",
            "Edge" => "msedge.exe",
            "Firefox" => "firefox.exe",
            _ => return false,
        };
        process_exists(exe)
    }

    fn capture(&self) -> Option<CaptureEntry> {
        let tab = match self.browser.as_str() {
            "Chrome" => latest_chrome_page()?,
            "Edge" => latest_edge_page()?,
            "Firefox" => latest_firefox_page()?,
            _ => return None,
        };

        capture_webpage_url(
            &tab.url,
            Some(&tab.title),
            &self.browser,
            "latest-history-entry",
        )
    }

    fn watch(&self, _engine: Arc<CaptureEngine>) {}
}

pub fn capture_webpage_url(
    url: &str,
    title_hint: Option<&str>,
    source_app: &str,
    capture_mode: &str,
) -> Option<CaptureEntry> {
    let trimmed_url = url.trim();
    if trimmed_url.is_empty() {
        return None;
    }

    let page = fetch_webpage(trimmed_url, title_hint);
    let data = readable_webpage_text(&page).into_bytes();
    let mut entry = CaptureEntry::new(
        data,
        CaptureKind::Webpage,
        CaptureSource::AppApi {
            app: source_app.to_string(),
        },
    )
    .with_title(&page.title);
    entry.meta.url = Some(page.url.clone());
    entry.meta.summary = Some(page.summary());
    entry.meta.app_name = Some(source_app.to_string());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), capture_mode.to_string());
    entry
        .meta
        .extra
        .insert("content_type".to_string(), page.content_type.clone());
    entry
        .meta
        .extra
        .insert("fetched_bytes".to_string(), page.bytes_read.to_string());
    if let Some(error) = &page.fetch_error {
        entry
            .meta
            .extra
            .insert("fetch_error".to_string(), error.clone());
    }
    Some(entry)
}

struct BrowserPage {
    title: String,
    url: String,
}

struct CapturedWebpage {
    url: String,
    title: String,
    content_type: String,
    text: String,
    bytes_read: usize,
    fetch_error: Option<String>,
}

impl CapturedWebpage {
    fn summary(&self) -> String {
        if !self.text.trim().is_empty() {
            return self.text.chars().take(200).collect();
        }
        self.fetch_error
            .clone()
            .unwrap_or_else(|| self.url.clone())
            .chars()
            .take(200)
            .collect()
    }
}

fn fetch_webpage(url: &str, title_hint: Option<&str>) -> CapturedWebpage {
    let fallback_title = title_hint
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Webpage")
        .to_string();

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return CapturedWebpage {
            url: url.to_string(),
            title: fallback_title,
            content_type: "text/plain".to_string(),
            text: format!("AEON cannot fetch non-http webpage URLs directly.\nURL: {url}"),
            bytes_read: 0,
            fetch_error: Some("unsupported URL scheme".to_string()),
        };
    }

    match fetch_http_body(url) {
        Ok(response) => {
            let text = response_text(&response.body, &response.content_type);
            let (title, body_text) = if response.content_type.contains("html") {
                html_to_readable_text(&text, &fallback_title)
            } else {
                (fallback_title.clone(), normalize_text(&text))
            };
            CapturedWebpage {
                url: response.final_url,
                title,
                content_type: response.content_type,
                text: body_text,
                bytes_read: response.body.len(),
                fetch_error: None,
            }
        }
        Err(error) => CapturedWebpage {
            url: url.to_string(),
            title: fallback_title,
            content_type: "text/plain".to_string(),
            text: format!("AEON could not fetch the webpage content.\nURL: {url}\nError: {error}"),
            bytes_read: 0,
            fetch_error: Some(error),
        },
    }
}

struct HttpBody {
    final_url: String,
    content_type: String,
    body: Vec<u8>,
}

fn fetch_http_body(url: &str) -> Result<HttpBody, String> {
    let url = url.to_string();
    std::thread::spawn(move || fetch_http_body_inner(&url))
        .join()
        .map_err(|_| "web fetch panicked".to_string())?
}

fn fetch_http_body_inner(url: &str) -> Result<HttpBody, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::limited(8))
        .user_agent("Mozilla/5.0 (compatible; AEON Capture/0.1)")
        .build()
        .map_err(|err| err.to_string())?;
    let response = client
        .get(url)
        .send()
        .map_err(|err| err.to_string())?
        .error_for_status()
        .map_err(|err| err.to_string())?;
    let final_url = response.url().to_string();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("text/plain")
        .to_string();
    let mut body = Vec::new();
    response
        .take(MAX_WEBPAGE_BYTES)
        .read_to_end(&mut body)
        .map_err(|err| err.to_string())?;
    Ok(HttpBody {
        final_url,
        content_type,
        body,
    })
}

fn response_text(body: &[u8], _content_type: &str) -> String {
    String::from_utf8_lossy(body).to_string()
}

fn html_to_readable_text(html: &str, fallback_title: &str) -> (String, String) {
    let document = Html::parse_document(html);
    let title = Selector::parse("title")
        .ok()
        .and_then(|selector| document.select(&selector).next())
        .map(|node| normalize_text(&node.text().collect::<Vec<_>>().join(" ")))
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| fallback_title.to_string());
    let body_text = Selector::parse("body")
        .ok()
        .map(|selector| {
            document
                .select(&selector)
                .flat_map(|node| node.text())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| document.root_element().text().collect::<Vec<_>>().join(" "));
    (title, normalize_text(&body_text))
}

fn normalize_text(text: &str) -> String {
    let mut out = String::new();
    let mut last_was_space = false;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
        if out.chars().count() >= MAX_WEBPAGE_TEXT_CHARS {
            break;
        }
    }
    out.trim().to_string()
}

fn readable_webpage_text(page: &CapturedWebpage) -> String {
    let mut out = format!(
        "# {}\n\nURL: {}\nContent-Type: {}\nCaptured: {}\n\n",
        page.title,
        page.url,
        page.content_type,
        super::util::now_ms()
    );
    if let Some(error) = &page.fetch_error {
        out.push_str("Fetch status: failed\n");
        out.push_str(error);
        out.push_str("\n\n");
    }
    out.push_str(&page.text);
    out
}

fn latest_chrome_page() -> Option<BrowserPage> {
    let history = chromium_history_path("Chrome")?;
    query_latest_history(
        &history,
        "select title, url from urls order by last_visit_time desc limit 1",
    )
}

fn latest_edge_page() -> Option<BrowserPage> {
    let history = chromium_history_path("Edge")?;
    query_latest_history(
        &history,
        "select title, url from urls order by last_visit_time desc limit 1",
    )
}

pub fn capture_browser_pages(browser: &str, limit: usize) -> Option<CaptureEntry> {
    let pages = recent_browser_pages(browser, limit);
    if pages.is_empty() {
        return None;
    }
    let mut text = format!(
        "# {browser} pages\n\nCaptured: {}\n\n",
        super::util::now_ms()
    );
    for (index, page) in pages.iter().enumerate() {
        text.push_str(&format!(
            "{}. {}\n   {}\n\n",
            index + 1,
            page.title,
            page.url
        ));
    }

    let mut entry = CaptureEntry::new(
        text.into_bytes(),
        CaptureKind::Webpage,
        CaptureSource::AppApi {
            app: browser.to_string(),
        },
    )
    .with_title(&format!("{browser} recent pages ({})", pages.len()))
    .with_summary(&pages[0].url)
    .with_app(browser);
    entry.meta.url = Some(pages[0].url.clone());
    entry
        .meta
        .extra
        .insert("capture_mode".to_string(), "browser-pages".to_string());
    entry
        .meta
        .extra
        .insert("page_count".to_string(), pages.len().to_string());
    Some(entry)
}

fn recent_browser_pages(browser: &str, limit: usize) -> Vec<BrowserPage> {
    let limit = limit.clamp(1, 100);
    let sql_chromium = format!(
        "select title, url from urls where url like 'http%' order by last_visit_time desc limit {limit}"
    );
    let sql_firefox = format!(
        "select title, url from moz_places where url like 'http%' order by last_visit_date desc limit {limit}"
    );
    match browser {
        "Chrome" => chromium_history_path("Chrome")
            .map(|path| query_history_pages(&path, &sql_chromium))
            .unwrap_or_default(),
        "Edge" => chromium_history_path("Edge")
            .map(|path| query_history_pages(&path, &sql_chromium))
            .unwrap_or_default(),
        "Firefox" => latest_firefox_history_path()
            .map(|path| query_history_pages(&path, &sql_firefox))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn chromium_history_path(browser: &str) -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
    match browser {
        "Chrome" => Some(
            local
                .join("Google")
                .join("Chrome")
                .join("User Data")
                .join("Default")
                .join("History"),
        ),
        "Edge" => Some(
            local
                .join("Microsoft")
                .join("Edge")
                .join("User Data")
                .join("Default")
                .join("History"),
        ),
        _ => None,
    }
}

fn latest_firefox_page() -> Option<BrowserPage> {
    let history = latest_firefox_history_path()?;
    query_latest_history(
        &history,
        "select title, url from moz_places where url is not null order by last_visit_date desc limit 1",
    )
}

fn latest_firefox_history_path() -> Option<PathBuf> {
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
    newest.map(|(_, history)| history)
}

fn query_latest_history(path: &Path, sql: &str) -> Option<BrowserPage> {
    query_history_pages(path, sql).into_iter().next()
}

fn query_history_pages(path: &Path, sql: &str) -> Vec<BrowserPage> {
    let temp = std::env::temp_dir().join(format!(
        "aeon-browser-history-{}.sqlite",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    if std::fs::copy(path, &temp).is_err() {
        return Vec::new();
    }
    let result = query_sqlite_pages(&temp, sql);
    let _ = std::fs::remove_file(temp);
    result
}

fn query_sqlite_pages(path: &Path, sql: &str) -> Vec<BrowserPage> {
    let output = Command::new("sqlite3")
        .arg("-readonly")
        .arg("-separator")
        .arg("\t")
        .arg(path)
        .arg(sql)
        .output()
        .ok();
    let Some(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let (title, url) = line.split_once('\t')?;
            let url = url.trim();
            if url.is_empty() {
                return None;
            }
            Some(BrowserPage {
                title: if title.trim().is_empty() {
                    "Webpage".to_string()
                } else {
                    title.trim().to_string()
                },
                url: url.to_string(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn webpage_capture_does_not_panic_inside_tokio_runtime() {
        let entry = capture_webpage_url(
            "http://127.0.0.1:9/aeon-unreachable",
            Some("Unreachable page"),
            "Test",
            "test",
        )
        .expect("failed fetches should still produce a capture entry");

        assert_eq!(entry.kind, CaptureKind::Webpage);
        assert!(entry.meta.extra.contains_key("fetch_error"));
        let data = String::from_utf8(entry.data).unwrap();
        assert!(data.contains("AEON could not fetch the webpage content."));
    }
}
