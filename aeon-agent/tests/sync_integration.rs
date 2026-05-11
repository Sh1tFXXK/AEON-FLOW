use aeon_store::{Blob, CIDStore, DeviceInfo, Identity, Platform};
use std::sync::Arc;

use aeon_agent::{engine::{device_id_from_name, SyncEngine}, state::SyncState};

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
async fn announce_on_empty_peer_set_is_safe() {
    let id = Identity::generate();
    let tmp = tempfile::tempdir().unwrap();
    let mut store = CIDStore::new(tmp.path().to_path_buf()).unwrap();
    let cid = store.put(Blob::new(b"hello aeon".to_vec(), "text/plain")).unwrap();
    let engine = Arc::new(SyncEngine::new(
        Arc::new(id),
        test_device([1;32], "solo"),
        store,
        SyncState::default(),
        tmp.path().join("state.json"),
        tmp.path().join("sync"),
    ));

    engine.announce(cid).await;
}

#[tokio::test]
async fn delete_announce_does_not_panic() {
    let id = Identity::generate();
    let tmp = tempfile::tempdir().unwrap();
    let store = CIDStore::new(tmp.path().to_path_buf()).unwrap();
    let engine = Arc::new(SyncEngine::new(
        Arc::new(id),
        test_device([3;32], "solo"),
        store,
        SyncState::default(),
        tmp.path().join("state.json"),
        tmp.path().join("sync"),
    ));
    engine.announce_delete("/tmp/a.txt".to_string(), [7u8; 32]).await;
}
