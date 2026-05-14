use crate::capture::{CaptureEntry, CaptureKind, CID};
use crate::store::{CaptureRecord, CaptureStore};
use aeon_store::Blob;
use std::io;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

pub struct CaptureEngine {
    store: Arc<Mutex<CaptureStore>>,
    tx: broadcast::Sender<CaptureEntry>,
    by: [u8; 32],
    device: [u8; 16],
}

impl CaptureEngine {
    pub fn new(store: Arc<Mutex<CaptureStore>>) -> Self {
        Self::new_with_identity(store, [0u8; 32], [0u8; 16])
    }

    pub fn new_with_identity(
        store: Arc<Mutex<CaptureStore>>,
        by: [u8; 32],
        device: [u8; 16],
    ) -> Self {
        let (tx, _) = broadcast::channel(256);
        CaptureEngine {
            store,
            tx,
            by,
            device,
        }
    }

    pub async fn capture(&self, mut entry: CaptureEntry) -> io::Result<CID> {
        let cid = entry.cid;
        self.stamp_identity(&mut entry);
        self.enrich(&mut entry);
        crate::apps::auto_wrap_capture_entry(&mut entry)
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

        {
            let mut store = self.store.lock().await;
            store.put(entry.clone())?;
        }

        let _ = self.tx.send(entry);
        Ok(cid)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CaptureEntry> {
        self.tx.subscribe()
    }

    pub async fn list(&self) -> Vec<CaptureRecord> {
        self.store.lock().await.list()
    }

    pub async fn get(&self, cid: &CID) -> io::Result<Option<CaptureEntry>> {
        self.store.lock().await.get(cid)
    }

    pub async fn raw(&self, cid: &CID) -> io::Result<Option<Blob>> {
        self.store.lock().await.raw(cid)
    }

    fn enrich(&self, entry: &mut CaptureEntry) {
        match &entry.kind {
            CaptureKind::Text | CaptureKind::Clipboard => {
                if let Ok(text) = std::str::from_utf8(&entry.data) {
                    if entry.meta.summary.is_none() {
                        entry.meta.summary = Some(text.chars().take(100).collect());
                    }
                    if entry.meta.title.is_none() {
                        let title = text
                            .lines()
                            .next()
                            .unwrap_or("文本片段")
                            .chars()
                            .take(50)
                            .collect::<String>();
                        entry.meta.title = Some(if title.trim().is_empty() {
                            "文本片段".to_string()
                        } else {
                            title
                        });
                    }
                }
            }
            CaptureKind::Code { language } => {
                if entry.meta.title.is_none() {
                    entry.meta.title = Some(format!("{language} 代码片段"));
                }
                if entry.meta.summary.is_none() {
                    if let Ok(text) = std::str::from_utf8(&entry.data) {
                        entry.meta.summary =
                            Some(text.lines().take(4).collect::<Vec<_>>().join("\n"));
                    }
                }
            }
            CaptureKind::Conversation => {
                if let Ok(text) = std::str::from_utf8(&entry.data) {
                    if let Ok(conv) = serde_json::from_str::<serde_json::Value>(text) {
                        if let Some(msgs) = conv["messages"].as_array() {
                            entry.meta.message_count = Some(msgs.len());
                            if entry.meta.summary.is_none() {
                                if let Some(last) = msgs.last() {
                                    let content = last["content"]
                                        .as_str()
                                        .unwrap_or("")
                                        .chars()
                                        .take(100)
                                        .collect::<String>();
                                    entry.meta.summary = Some(content);
                                }
                            }
                        }
                    }
                }
            }
            CaptureKind::Webpage => {
                if entry.meta.summary.is_none() {
                    if let Ok(text) = std::str::from_utf8(&entry.data) {
                        entry.meta.summary = Some(text.chars().take(120).collect());
                    }
                }
            }
            _ => {}
        }
    }

    fn stamp_identity(&self, entry: &mut CaptureEntry) {
        if entry.by == [0u8; 32] {
            entry.by = self.by;
        }
        if entry.device == [0u8; 16] {
            entry.device = self.device;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureKind, CaptureSource};
    use aeon_store::CIDStore;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-capture-engine-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn capture_enriches_text_and_broadcasts() {
        let dir = temp_dir();
        let store = CaptureStore::new(
            CIDStore::new(dir.join("store")).unwrap(),
            dir.join("index.json"),
        )
        .unwrap();
        let engine = CaptureEngine::new(Arc::new(Mutex::new(store)));
        let mut rx = engine.subscribe();

        let cid = engine
            .capture(CaptureEntry::new(
                "第一行\n第二行".as_bytes().to_vec(),
                CaptureKind::Clipboard,
                CaptureSource::Clipboard,
            ))
            .await
            .unwrap();

        let event = rx.recv().await.unwrap();
        assert_eq!(event.cid, cid);
        assert_eq!(event.meta.title.as_deref(), Some("第一行"));
        assert_eq!(engine.list().await.len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }
}
