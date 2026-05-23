use crate::{
    pack_cids, unpack_cids, Account, Blob, CIDStore, Context, DataDescriptor, DataEvent, DataKind,
    Message, Node, SyncEngine, SyncMessage, Thread, CONTEXT_MIME, MESSAGE_MIME, THREAD_MIME,
};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;

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

#[test]
fn packed_cids_roundtrip() {
    let a = Blob::from_text("a").cid;
    let b = Blob::from_text("b").cid;

    let packed = pack_cids(&[a, b]);
    let unpacked = unpack_cids(&packed).unwrap();

    assert_eq!(unpacked, vec![a, b]);
}

#[test]
fn sync_message_roundtrips_through_json() {
    let blob = Blob::from_text("sync me");
    let message = SyncMessage::Data {
        blob: blob.clone(),
        node: None,
    };

    let encoded = serde_json::to_vec(&message).unwrap();
    let decoded: SyncMessage = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded, message);
}

#[test]
fn sync_announce_transfers_missing_blob() {
    let root_a = TempRoot::new("sync-a");
    let root_b = TempRoot::new("sync-b");
    let mut store_a = CIDStore::new(root_a.path()).unwrap();
    let cid = store_a.put(Blob::from_text("cross-device data")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let root_b_path = root_b.path();

    let receiver = thread::spawn(move || {
        let store_b = CIDStore::new(root_b_path).unwrap();
        let mut engine_b = SyncEngine::new(store_b, [2u8; 16]);
        engine_b.listen_once_on(listener).unwrap()
    });

    let mut engine_a = SyncEngine::new(store_a, [1u8; 16]);
    let sent = engine_a.announce_to(cid, &addr.to_string()).unwrap();
    let received = receiver.join().unwrap();

    assert_eq!(sent.sent, 1);
    assert_eq!(received.requested, 1);
    assert_eq!(received.received, 1);

    let mut reloaded_b = CIDStore::new(root_b.path()).unwrap();
    let blob = reloaded_b.get(&cid).unwrap().unwrap();
    assert_eq!(blob.as_text(), Some("cross-device data"));
}

#[test]
fn sync_listener_serves_multiple_announcements() {
    let root_a = TempRoot::new("sync-multi-a");
    let root_b = TempRoot::new("sync-multi-b");
    let mut store_a = CIDStore::new(root_a.path()).unwrap();
    let first = store_a.put(Blob::from_text("first")).unwrap();
    let second = store_a.put(Blob::from_text("second")).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let root_b_path = root_b.path();

    let receiver = thread::spawn(move || {
        let store_b = CIDStore::new(root_b_path).unwrap();
        let mut engine_b = SyncEngine::new(store_b, [2u8; 16]);
        engine_b.listen_n_on(listener, 2).unwrap()
    });

    let mut engine_a = SyncEngine::new(store_a, [1u8; 16]);
    engine_a.announce_to(first, &addr.to_string()).unwrap();
    engine_a.announce_to(second, &addr.to_string()).unwrap();
    let received = receiver.join().unwrap();

    assert_eq!(received.requested, 2);
    assert_eq!(received.received, 2);

    let mut reloaded_b = CIDStore::new(root_b.path()).unwrap();
    assert_eq!(
        reloaded_b.get(&first).unwrap().unwrap().as_text(),
        Some("first")
    );
    assert_eq!(
        reloaded_b.get(&second).unwrap().unwrap().as_text(),
        Some("second")
    );
}

#[test]
fn account_id_is_public_key_hash() {
    let public_key = [7u8; 32];
    let account = Account::from_public_key("alice", public_key);

    assert_eq!(account.id, *blake3::hash(&public_key).as_bytes());
    assert_eq!(account.display_name, "alice");
}

#[test]
fn context_tracks_members_nodes_and_history() {
    let alice = Account::from_public_key("alice", [1u8; 32]);
    let bob = Account::from_public_key("bob", [2u8; 32]);
    let old_node = Blob::from_text("v1").cid;
    let new_node = Blob::from_text("v2").cid;
    let mut context = Context::new("project", alice.id);

    context.add_member(bob.id, 10);
    context.add_node(old_node, alice.id, 11).unwrap();
    context.update_node(old_node, new_node, bob.id, 12).unwrap();
    context.message("reviewed", bob.id, 13).unwrap();

    assert!(context.is_member(&alice.id));
    assert!(context.is_member(&bob.id));
    assert_eq!(context.nodes, vec![new_node]);
    assert_eq!(context.events.len(), 5);
}

#[test]
fn context_rejects_non_member_edits() {
    let alice = Account::from_public_key("alice", [1u8; 32]);
    let eve = Account::from_public_key("eve", [9u8; 32]);
    let mut context = Context::new("project", alice.id);

    let err = context.add_node(Blob::from_text("secret").cid, eve.id, 1);

    assert!(err.is_err());
    assert!(context.nodes.is_empty());
}

#[test]
fn context_roundtrips_as_blob() {
    let alice = Account::from_public_key("alice", [1u8; 32]);
    let mut context = Context::new("project", alice.id);
    context
        .add_node(Blob::from_text("design").cid, alice.id, 2)
        .unwrap();

    let blob = context.to_blob().unwrap();
    let decoded = Context::from_blob(&blob).unwrap();

    assert_eq!(blob.mime, CONTEXT_MIME);
    assert_eq!(decoded, context);
}

#[test]
fn context_blob_syncs_between_devices() {
    let alice = Account::from_public_key("alice", [1u8; 32]);
    let mut context = Context::new("shared", alice.id);
    context
        .add_node(Blob::from_text("shared note").cid, alice.id, 2)
        .unwrap();

    let root_a = TempRoot::new("ctx-sync-a");
    let root_b = TempRoot::new("ctx-sync-b");
    let mut store_a = CIDStore::new(root_a.path()).unwrap();
    let context_cid = store_a.put(context.to_blob().unwrap()).unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let root_b_path = root_b.path();

    let receiver = thread::spawn(move || {
        let store_b = CIDStore::new(root_b_path).unwrap();
        let mut engine_b = SyncEngine::new(store_b, [4u8; 16]);
        engine_b.listen_once_on(listener).unwrap()
    });

    let mut engine_a = SyncEngine::new(store_a, [3u8; 16]);
    engine_a
        .announce_to(context_cid, &addr.to_string())
        .unwrap();
    receiver.join().unwrap();

    let mut reloaded_b = CIDStore::new(root_b.path()).unwrap();
    let blob = reloaded_b.get(&context_cid).unwrap().unwrap();
    let decoded = Context::from_blob(&blob).unwrap();

    assert_eq!(decoded.name, "shared");
    assert_eq!(decoded.nodes, context.nodes);
    assert_eq!(decoded.events, context.events);
}

#[test]
fn data_kind_classifies_common_extensions() {
    assert_eq!(
        DataKind::from_path_and_mime(Some(std::path::Path::new("note.md")), "text/plain"),
        DataKind::Markdown
    );
    assert_eq!(
        DataKind::from_path_and_mime(Some(std::path::Path::new("main.rs")), "text/x-rust"),
        DataKind::Code {
            language: "rust".to_string()
        }
    );
    assert_eq!(
        DataKind::from_path_and_mime(
            Some(std::path::Path::new("program.aeon")),
            "application/x-aeon-program",
        ),
        DataKind::VMProgram
    );
}

#[test]
fn binary_image_roundtrip_is_exact() {
    let root = TempRoot::new("image");
    let mut store = CIDStore::new(root.path()).unwrap();
    let bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 1, 2, 3, 4];
    let blob = Blob::new(bytes.clone(), "image/png");

    let cid = store.put(blob.clone()).unwrap();
    let retrieved = store.get(&cid).unwrap().unwrap();
    let descriptor = DataDescriptor::from_blob(Some(std::path::Path::new("shot.png")), &retrieved);

    assert_eq!(retrieved.data, bytes);
    assert_eq!(descriptor.blob_cid, blob.cid);
    assert!(matches!(descriptor.kind, DataKind::Image { ref format, .. } if format == "png"));
}

#[test]
fn conversation_message_and_thread_roundtrip() {
    let content = Blob::from_text("hello bob");
    let message = Message::new("thread-1", "alice", content.cid, None, 1);
    let mut thread = Thread::new(
        "thread-1",
        vec!["alice".to_string(), "bob".to_string()],
        Some("ctx-1".to_string()),
    );
    thread.add_message(&message);

    let message_blob = message.to_blob().unwrap();
    let thread_blob = thread.to_blob().unwrap();

    assert_eq!(message_blob.mime, MESSAGE_MIME);
    assert_eq!(thread_blob.mime, THREAD_MIME);
    assert_eq!(Message::from_blob(&message_blob).unwrap(), message);
    assert_eq!(Thread::from_blob(&thread_blob).unwrap(), thread);
}

#[test]
fn conversation_message_syncs_between_devices() {
    let root_a = TempRoot::new("chat-a");
    let root_b = TempRoot::new("chat-b");
    let mut store_a = CIDStore::new(root_a.path()).unwrap();
    let content = Blob::from_text("arrived on device B");
    let content_cid = store_a.put(content.clone()).unwrap();
    let message = Message::new("thread-sync", "alice", content_cid, None, 42);
    let message_cid = store_a.put(message.to_blob().unwrap()).unwrap();

    sync_one_blob(root_a.path(), root_b.path(), content_cid);
    sync_one_blob(root_a.path(), root_b.path(), message_cid);

    let mut store_b = CIDStore::new(root_b.path()).unwrap();
    let message_blob = store_b.get(&message_cid).unwrap().unwrap();
    let decoded = Message::from_blob(&message_blob).unwrap();
    let content_blob = store_b.get(&decoded.content_cid).unwrap().unwrap();

    assert_eq!(decoded, message);
    assert_eq!(content_blob.as_text(), Some("arrived on device B"));
}

fn sync_one_blob(root_a: PathBuf, root_b: PathBuf, cid: crate::CID) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();

    let receiver = thread::spawn(move || {
        let store_b = CIDStore::new(root_b).unwrap();
        let mut engine_b = SyncEngine::new(store_b, [8u8; 16]);
        engine_b.listen_once_on(listener).unwrap()
    });

    let store_a = CIDStore::new(root_a).unwrap();
    let mut engine_a = SyncEngine::new(store_a, [7u8; 16]);
    engine_a.announce_to(cid, &addr.to_string()).unwrap();
    receiver.join().unwrap();
}
