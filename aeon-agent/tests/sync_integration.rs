use aeon_store::{Blob, CIDStore, DeviceInfo, Identity, Platform};
use std::sync::Arc;

use aeon_agent::{engine::{device_id_from_name, SyncEngine}};

fn test_device(identity_id: [u8; 32], name: &str) -> DeviceInfo {
    DeviceInfo {
        device_id: device_id_from_name(name),
        identity_id,
        name: name.to_string(),
        platform: Platform::current(),
        last_seen: 0,
    }
}

#[tokio::test]
async fn have_want_data_roundtrip_syncs_blob() {
    let id1 = Identity::generate();
    let id2 = Identity::generate();

    let dir1 = tempfile::tempdir().unwrap();
    let dir2 = tempfile::tempdir().unwrap();
    let mut s1 = CIDStore::new(dir1.path().to_path_buf()).unwrap();
    let s2 = CIDStore::new(dir2.path().to_path_buf()).unwrap();

    let blob = Blob::from_bytes(b"hello aeon".to_vec(), "text/plain");
    let cid = s1.put(blob).unwrap();

    let e1 = Arc::new(SyncEngine::new(Arc::new(id1), test_device([1;32], "a"), s1));
    let e2 = Arc::new(SyncEngine::new(Arc::new(id2), test_device([2;32], "b"), s2));

    let l1 = e1.clone();
    tokio::spawn(async move { let _ = l1.listen("127.0.0.1:9787").await; });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let _ = e2.clone().connect("127.0.0.1:9787").await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    e1.announce(cid).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let got = e2.store.lock().unwrap().get(&cid).unwrap();
    assert!(got.is_some(), "peer should receive blob after Have/Want/Data flow");
}

#[tokio::test]
async fn delete_announce_does_not_panic() {
    let id = Identity::generate();
    let store = CIDStore::new(tempfile::tempdir().unwrap().path().to_path_buf()).unwrap();
    let engine = Arc::new(SyncEngine::new(Arc::new(id), test_device([3;32], "solo"), store));
    engine.announce_delete("/tmp/a.txt".to_string(), [7u8; 32]).await;
}
