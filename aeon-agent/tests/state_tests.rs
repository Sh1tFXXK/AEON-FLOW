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
