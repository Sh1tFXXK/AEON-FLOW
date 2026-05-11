use aeon_agent::state::SyncState;

#[test]
fn collab_metrics_baseline() {
    let mut st = SyncState::default();
    let m = st.metric_mut("abc");
    m.applied_patches += 2;
    m.compacted_snapshots += 1;
    assert_eq!(st.collab_metrics.get("abc").unwrap().applied_patches, 2);
    assert_eq!(st.collab_metrics.get("abc").unwrap().compacted_snapshots, 1);
}
