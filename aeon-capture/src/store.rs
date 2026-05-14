use crate::capture::{CaptureEntry, CaptureKind, CaptureMetadata, CaptureSource, CID};
use aeon_store::{Blob, CIDStore};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CaptureRecord {
    pub cid: CID,
    pub kind: CaptureKind,
    pub meta: CaptureMetadata,
    pub source: CaptureSource,
    pub captured_at: u64,
    pub by: [u8; 32],
    pub device: [u8; 16],
    pub size: usize,
    pub mime: String,
}

pub struct CaptureStore {
    cid_store: CIDStore,
    index_path: PathBuf,
    records: Vec<CaptureRecord>,
}

impl CaptureStore {
    pub fn new(cid_store: CIDStore, index_path: PathBuf) -> io::Result<Self> {
        let records = read_index(&index_path)?;
        Ok(Self {
            cid_store,
            index_path,
            records,
        })
    }

    pub fn open_default() -> io::Result<Self> {
        let aeon_dir = default_aeon_dir();
        std::fs::create_dir_all(&aeon_dir)?;
        Self::new(
            CIDStore::new(aeon_dir.join("store"))?,
            aeon_dir.join("capture-index.json"),
        )
    }

    pub fn put(&mut self, entry: CaptureEntry) -> io::Result<CID> {
        let cid = entry.cid;
        let mime = entry.mime();
        let size = entry.data.len();
        self.cid_store.put(Blob::new(entry.data, &mime))?;

        let record = CaptureRecord {
            cid,
            kind: entry.kind,
            meta: entry.meta,
            source: entry.source,
            captured_at: entry.captured_at,
            by: entry.by,
            device: entry.device,
            size,
            mime,
        };

        self.upsert_record(record)?;
        Ok(cid)
    }

    pub fn list(&self) -> Vec<CaptureRecord> {
        let mut records = self.records.clone();
        records.sort_by(|a, b| b.captured_at.cmp(&a.captured_at).then(a.cid.cmp(&b.cid)));
        records
    }

    pub fn get(&mut self, cid: &CID) -> io::Result<Option<CaptureEntry>> {
        let Some(record) = self
            .records
            .iter()
            .find(|record| &record.cid == cid)
            .cloned()
        else {
            return Ok(None);
        };
        let Some(blob) = self.cid_store.get(cid)? else {
            return Ok(None);
        };

        Ok(Some(CaptureEntry {
            cid: record.cid,
            kind: record.kind,
            data: blob.data,
            meta: record.meta,
            source: record.source,
            captured_at: record.captured_at,
            by: record.by,
            device: record.device,
        }))
    }

    pub fn raw(&mut self, cid: &CID) -> io::Result<Option<Blob>> {
        self.cid_store.get(cid)
    }

    fn upsert_record(&mut self, record: CaptureRecord) -> io::Result<()> {
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.cid == record.cid)
        {
            *existing = record;
        } else {
            self.records.push(record);
        }
        self.records
            .sort_by(|a, b| b.captured_at.cmp(&a.captured_at).then(a.cid.cmp(&b.cid)));
        write_index(&self.index_path, &self.records)
    }
}

fn read_index(path: &Path) -> io::Result<Vec<CaptureRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes).unwrap_or_default())
}

fn write_index(path: &Path, records: &[CaptureRecord]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(records)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    std::fs::write(path, bytes)
}

fn default_aeon_dir() -> PathBuf {
    std::env::var_os("AEON_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".aeon")))
        .unwrap_or_else(|| PathBuf::from(".aeon"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::{CaptureKind, CaptureSource};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-capture-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn stores_raw_data_and_index_record() {
        let dir = temp_dir();
        let cid_store = CIDStore::new(dir.join("store")).unwrap();
        let mut store = CaptureStore::new(cid_store, dir.join("index.json")).unwrap();
        let entry = CaptureEntry::new(
            b"hello".to_vec(),
            CaptureKind::Clipboard,
            CaptureSource::Clipboard,
        );
        let cid = entry.cid;

        store.put(entry).unwrap();
        let restored = store.get(&cid).unwrap().unwrap();

        assert_eq!(restored.data, b"hello");
        assert_eq!(store.list()[0].kind, CaptureKind::Clipboard);
        let _ = std::fs::remove_dir_all(dir);
    }
}
