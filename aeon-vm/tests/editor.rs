use aeon_vm::editor::{Inspector, PatchSet, SnapshotEditor};
use aeon_vm::program::programs;
use aeon_vm::snapshot::Snapshot;
use aeon_vm::vm::VMState;

fn fib_snap(steps: usize) -> Snapshot {
    let program = programs::fibonacci(10);
    let mut state = VMState::new(&program);
    state.run_bounded(&program, steps);
    Snapshot::capture(&state)
}

#[test]
fn empty_patchset_has_zero_len() {
    let patchset = PatchSet::empty("noop");
    assert_eq!(patchset.len(), 0);
    assert!(patchset.is_empty());
}

#[test]
fn editor_set_reg_applies_change() {
    let snap = fib_snap(5);
    let patched = SnapshotEditor::new(&snap, "set register")
        .set_reg(0, 99)
        .unwrap()
        .build()
        .apply(&snap)
        .unwrap();

    assert_eq!(patched.regs[0], 99);
}

#[test]
fn set_reg_reverse_restores_original() {
    let snap = fib_snap(5);
    let patchset = SnapshotEditor::new(&snap, "set register")
        .set_reg(0, 99)
        .unwrap()
        .build();
    let patched = patchset.apply(&snap).unwrap();
    let restored = patchset.reverse().apply(&patched).unwrap();

    assert_eq!(restored.regs, snap.regs);
}

#[test]
fn set_reg_rejects_out_of_range_index() {
    let mut snap = fib_snap(5);
    snap.regs = vec![0];
    let err = SnapshotEditor::new(&snap, "bad register").set_reg(2, 1);

    assert!(err.is_err());
}

#[test]
fn set_pc_applies_change() {
    let snap = fib_snap(5);
    let patched = SnapshotEditor::new(&snap, "set pc")
        .set_pc(3)
        .unwrap()
        .build()
        .apply(&snap)
        .unwrap();

    assert_eq!(patched.pc, 3);
}

#[test]
fn set_call_stack_entry_applies_change() {
    let mut snap = fib_snap(5);
    snap.call_stack = vec![4, 8];

    let patched = SnapshotEditor::new(&snap, "set call stack")
        .set_call_stack_entry(1, 12)
        .unwrap()
        .build()
        .apply(&snap)
        .unwrap();

    assert_eq!(patched.call_stack, vec![4, 12]);
}

#[test]
fn set_call_stack_entry_rejects_out_of_range_index() {
    let snap = fib_snap(5);
    let err = SnapshotEditor::new(&snap, "bad call stack").set_call_stack_entry(0, 9);
    assert!(err.is_err());
}

#[test]
fn patchset_applies_multiple_patches_in_order() {
    let snap = fib_snap(5);
    let patched = SnapshotEditor::new(&snap, "multiple")
        .set_reg(0, 10)
        .unwrap()
        .set_reg(1, 20)
        .unwrap()
        .set_pc(6)
        .unwrap()
        .build()
        .apply(&snap)
        .unwrap();

    assert_eq!(patched.regs[0], 10);
    assert_eq!(patched.regs[1], 20);
    assert_eq!(patched.pc, 6);
}

#[test]
fn patchset_reverse_restores_multiple_patches() {
    let snap = fib_snap(5);
    let patchset = SnapshotEditor::new(&snap, "multiple")
        .set_reg(0, 10)
        .unwrap()
        .set_reg(1, 20)
        .unwrap()
        .set_pc(6)
        .unwrap()
        .build();
    let patched = patchset.apply(&snap).unwrap();
    let restored = patchset.reverse().apply(&patched).unwrap();

    assert_eq!(restored.regs, snap.regs);
    assert_eq!(restored.pc, snap.pc);
}

#[test]
fn patchset_roundtrips_through_bytes() {
    let snap = fib_snap(5);
    let patchset = SnapshotEditor::new(&snap, "serialize")
        .set_reg(0, 77)
        .unwrap()
        .build();

    let bytes = patchset.to_bytes();
    let decoded = PatchSet::from_bytes(&bytes).unwrap();

    assert_eq!(decoded.description, "serialize");
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded.apply(&snap).unwrap().regs[0], 77);
}

#[test]
fn inspector_diff_reports_reg_changes() {
    let snap = fib_snap(5);
    let patched = SnapshotEditor::new(&snap, "set register")
        .set_reg(0, 99)
        .unwrap()
        .build()
        .apply(&snap)
        .unwrap();

    assert!(Inspector::diff(&snap, &patched)
        .iter()
        .any(|line| line.contains("r0")));
}

#[test]
fn inspector_diff_reports_pc_changes() {
    let snap = fib_snap(5);
    let patched = SnapshotEditor::new(&snap, "set pc")
        .set_pc(9)
        .unwrap()
        .build()
        .apply(&snap)
        .unwrap();

    assert!(Inspector::diff(&snap, &patched)
        .iter()
        .any(|line| line.contains("pc")));
}

#[test]
fn inspector_diff_reports_call_stack_changes() {
    let mut snap = fib_snap(5);
    snap.call_stack = vec![1];
    let mut changed = snap.clone();
    changed.call_stack = vec![1, 2];

    assert!(Inspector::diff(&snap, &changed)
        .iter()
        .any(|line| line.contains("call_stack")));
}

#[test]
fn inspector_diff_is_empty_for_identical_snapshots() {
    let snap = fib_snap(5);
    assert!(Inspector::diff(&snap, &snap).is_empty());
}

#[test]
fn editor_build_preserves_description() {
    let snap = fib_snap(5);
    let patchset = SnapshotEditor::new(&snap, "important edit")
        .set_reg(0, 1)
        .unwrap()
        .build();

    assert_eq!(patchset.description, "important edit");
}

#[test]
fn patchset_len_counts_patches() {
    let snap = fib_snap(5);
    let patchset = SnapshotEditor::new(&snap, "count")
        .set_reg(0, 1)
        .unwrap()
        .set_reg(1, 2)
        .unwrap()
        .build();

    assert_eq!(patchset.len(), 2);
}

#[test]
fn set_heap_byte_errors_before_heap_support() {
    let snap = fib_snap(5);
    assert!(SnapshotEditor::new(&snap, "heap")
        .set_heap_byte(0, 1)
        .is_err());
}

#[test]
fn set_heap_range_errors_before_heap_support() {
    let snap = fib_snap(5);
    assert!(SnapshotEditor::new(&snap, "heap")
        .set_heap_range(0, vec![1, 2])
        .is_err());
}

#[test]
fn set_heap_str_errors_before_heap_support() {
    let snap = fib_snap(5);
    assert!(SnapshotEditor::new(&snap, "heap")
        .set_heap_str(0, "hello")
        .is_err());
}

#[test]
fn set_heap_u64_errors_before_heap_support() {
    let snap = fib_snap(5);
    assert!(SnapshotEditor::new(&snap, "heap")
        .set_heap_u64(0, 42)
        .is_err());
}
