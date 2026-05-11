use aeon_agent::state::SyncState;

#[test]
fn stable_doc_id_mapping() {
    let mut s = SyncState::default();
    let a = s.collab_doc_for_path("/tmp/a.txt");
    let b = s.collab_doc_for_path("/tmp/a.txt");
    assert_eq!(a, b);
}

#[test]
fn tombstone_retention_and_map() {
    let mut s = SyncState::default();
    for i in 0..10 {
        s.add_tombstone(format!("/tmp/{i}.txt"), [i as u8; 32], i);
    }
    let map = s.tombstone_map();
    assert_eq!(map.len(), 10);
}


#[test]
fn trusted_keys_are_tofu_and_stable() {
    let mut s = SyncState::default();
    let id = [9u8; 32];
    let key1 = vec![1u8; 32];
    let key2 = vec![2u8; 32];

    assert!(s.trust_key(id, &key1));
    assert_eq!(s.trusted_key_for(id).unwrap(), key1);
    assert!(s.trust_key(id, &key1));
    assert!(!s.trust_key(id, &key2));
}


#[test]
fn file_ingest_record_tracks_identity_and_device() {
    let mut s = SyncState::default();
    let path = "/tmp/demo.txt";
    s.record_file_ingest(path, [3u8; 32], [4u8; 32], [5u8; 16], "text/plain", 42);

    let rec = s.file_records.get(path).expect("record");
    assert_eq!(rec.cid.len(), 64);
    assert_eq!(rec.identity_id.len(), 64);
    assert_eq!(rec.device_id.len(), 32);
    assert_eq!(rec.mime, "text/plain");
    assert_eq!(rec.observed_at, 42);

    s.remove_file_record(path);
    assert!(s.file_records.get(path).is_none());
}


#[test]
fn tunnel_status_is_persistable_shape() {
    let mut s = SyncState::default();
    s.set_tunnel_status("cloudflare".to_string(), "edge.example.com:443".to_string(), true, "healthy".to_string());
    let st = s.tunnel_status.expect("tunnel status");
    assert_eq!(st.provider, "cloudflare");
    assert_eq!(st.endpoint, "edge.example.com:443");
    assert!(st.healthy);
    assert_eq!(st.state, "healthy");
    assert!(st.updated_at > 0);
}
