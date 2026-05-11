use aeon_agent::collab::CollabDoc;

#[test]
fn merges_two_devices_without_conflicts() {
    let mut desktop = CollabDoc::new("notes");
    let baseline = desktop.snapshot_bytes();
    let mut phone = CollabDoc::from_snapshot(&baseline).expect("snapshot load");

    desktop.insert(5, " from desktop");
    phone.insert(5, " from phone");

    let desktop_updates = desktop.snapshot_bytes();
    let phone_updates = phone.snapshot_bytes();

    desktop.merge(&phone_updates).expect("merge phone->desktop");
    phone.merge(&desktop_updates).expect("merge desktop->phone");

    assert_eq!(desktop.content(), phone.content());
    assert!(desktop.content().contains("desktop"));
    assert!(desktop.content().contains("phone"));
}

#[test]
fn out_of_range_insert_appends_at_end() {
    let mut doc = CollabDoc::new("abc");
    doc.insert(99, "!");
    assert_eq!(doc.content(), "abc!");
}
