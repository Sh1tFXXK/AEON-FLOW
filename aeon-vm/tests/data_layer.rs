use aeon_store::{Blob, CIDStore, Node};
use aeon_vm::data_layer::{load_program, load_snapshot, store_patchset, store_vm_snapshot};
use aeon_vm::editor::{PatchSet, SnapshotEditor};
use aeon_vm::program::programs;
use aeon_vm::vm::StepResult;
use aeon_vm::{ProgramStore, Snapshot, VMState};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new(name: &str) -> Self {
        let suffix = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "aeon-vm-data-{name}-{}-{suffix}",
            std::process::id()
        ));
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
fn vm_snapshot_and_program_roundtrip_through_cid_store() {
    let root = TempRoot::new("snapshot");
    let mut cid_store = CIDStore::new(root.path()).unwrap();
    let program = programs::fibonacci(10);
    let mut state = VMState::new(&program);
    assert_eq!(state.step(&program), StepResult::Ok);
    assert_eq!(state.step(&program), StepResult::Ok);

    let stored = store_vm_snapshot(&mut cid_store, &state, &program, "alice@laptop").unwrap();

    assert!(cid_store.has(&stored.snapshot_cid));
    assert!(cid_store.has(&stored.program_cid));
    assert!(cid_store.has(&stored.snapshot_node_blob_cid));
    assert!(cid_store.has(&stored.program_node_blob_cid));

    let loaded_program = load_program(&mut cid_store, stored.program_cid).unwrap();
    let loaded_snapshot = load_snapshot(&mut cid_store, stored.snapshot_cid).unwrap();
    let program_store = ProgramStore::new();
    program_store.add(loaded_program.clone());
    let restored = loaded_snapshot.restore(&program_store).unwrap();

    assert_eq!(loaded_program.id(), program.id());
    assert_eq!(restored.pc, state.pc);
    assert_eq!(restored.steps, state.steps);
}

#[test]
fn patchset_is_stored_as_cid_data_with_snapshot_link() {
    let root = TempRoot::new("patch");
    let mut cid_store = CIDStore::new(root.path()).unwrap();
    let program = programs::fibonacci(10);
    let state = VMState::new(&program);
    let snap = Snapshot::capture(&state);
    let snapshot_cid = cid_store
        .put(Blob::new(
            snap.to_bytes(),
            aeon_vm::data_layer::VM_SNAPSHOT_MIME,
        ))
        .unwrap();
    let patchset = SnapshotEditor::new(&snap, "set r0 for migration")
        .set_reg(0, 3)
        .unwrap()
        .build();

    let stored = store_patchset(&mut cid_store, &patchset, snapshot_cid, "alice@laptop").unwrap();
    let patch_blob = cid_store.get(&stored.patchset_cid).unwrap().unwrap();
    let decoded = PatchSet::from_bytes(&patch_blob.data).unwrap();
    let node_blob = cid_store.get(&stored.patch_node_blob_cid).unwrap().unwrap();
    let node: Node = serde_json::from_slice(&node_blob.data).unwrap();

    assert_eq!(decoded.description, patchset.description);
    assert_eq!(node.kind, "vm-patchset");
    assert_eq!(node.links[0].target_cid, snapshot_cid);
    assert_eq!(node.links[0].label, "applies-to");
}
