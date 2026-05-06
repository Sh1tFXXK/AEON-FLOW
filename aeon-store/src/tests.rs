use crate::{Blob, CIDStore, DataEvent, Node};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("aeon-store-{name}-{}-{suffix}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Self(root)
    }

    fn path(&self) -> PathBuf {
        self.0.clone()
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn put_and_get_roundtrip() {
    let root = TempRoot::new("roundtrip");
    let mut store = CIDStore::new(root.path()).unwrap();
    let blob = Blob::from_text("hello world");

    let cid = store.put(blob.clone()).unwrap();
    let retrieved = store.get(&cid).unwrap().unwrap();

    assert_eq!(retrieved.data, blob.data);
    assert_eq!(retrieved.mime, "text/plain");
    assert_eq!(retrieved.as_text(), Some("hello world"));
}

#[test]
fn same_content_same_cid() {
    let root = TempRoot::new("dedupe");
    let mut store = CIDStore::new(root.path()).unwrap();

    let c1 = store.put(Blob::from_text("hello")).unwrap();
    let c2 = store.put(Blob::from_text("hello")).unwrap();

    assert_eq!(c1, c2);
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn different_content_different_cid() {
    let root = TempRoot::new("different");
    let mut store = CIDStore::new(root.path()).unwrap();

    let c1 = store.put(Blob::from_text("hello")).unwrap();
    let c2 = store.put(Blob::from_text("world")).unwrap();

    assert_ne!(c1, c2);
    assert_eq!(store.list().unwrap().len(), 2);
}

#[test]
fn persists_to_disk_and_reloads() {
    let root = TempRoot::new("persist");
    let cid = {
        let mut store = CIDStore::new(root.path()).unwrap();
        store.put(Blob::from_text("persist me")).unwrap()
    };

    let mut store2 = CIDStore::new(root.path()).unwrap();
    let blob = store2.get(&cid).unwrap().unwrap();

    assert_eq!(blob.as_text(), Some("persist me"));
}

#[test]
fn node_links_two_blobs() {
    let source = Blob::from_text("design note");
    let target = Blob::from_text("screenshot bytes");
    let unlinked = Node::new(source.cid, "note", "alice@laptop").with_name("design.md");

    let linked = unlinked.clone().link_to(target.cid, "references");

    assert_ne!(unlinked.cid, linked.cid);
    assert_eq!(linked.links.len(), 1);
    assert_eq!(linked.links[0].target_cid, target.cid);
    assert_eq!(linked.links[0].label, "references");
}

#[test]
fn event_log_records_all_operations() {
    let blob = Blob::from_text("hello");
    let node = Node::new(blob.cid, "file", "alice@laptop").with_name("hello.txt");
    let events = vec![
        DataEvent::BlobAdded {
            cid: blob.cid,
            mime: blob.mime.clone(),
            size_bytes: blob.data.len(),
            by: "alice@laptop".to_string(),
        },
        DataEvent::NodeCreated {
            node_cid: node.cid,
            kind: node.kind.clone(),
            name: node.name.clone(),
            by: node.created_by.clone(),
        },
    ];

    let encoded = serde_json::to_vec(&events).unwrap();
    let decoded: Vec<DataEvent> = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded, events);
}
