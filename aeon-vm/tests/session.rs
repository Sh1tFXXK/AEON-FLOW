use aeon_vm::editor::{PatchSet, SnapshotEditor};
use aeon_vm::eventlog::AeonEvent;
use aeon_vm::program::programs;
use aeon_vm::session::{ContextRegistry, Lamport, SessionId, SharedContext};
use aeon_vm::snapshot::Snapshot;
use aeon_vm::store::ProgramStore;
use aeon_vm::vm::VMState;

fn alice() -> SessionId {
    SessionId::new("alice", "laptop", "conv-1")
}

fn bob() -> SessionId {
    SessionId::new("bob", "desktop", "conv-2")
}

fn fib_snap(steps: usize) -> Snapshot {
    let program = programs::fibonacci(10);
    let mut state = VMState::new(&program);
    state.run_bounded(&program, steps);
    Snapshot::capture(&state)
}

fn reg_patch(snap: &Snapshot, reg: u8, val: u64, desc: &str) -> PatchSet {
    SnapshotEditor::new(snap, desc)
        .set_reg(reg, val)
        .unwrap()
        .build()
}

#[test]
fn session_id_format() {
    let id = SessionId::new("alice", "laptop", "conv-1");
    assert_eq!(id.display(), "alice@laptop/conv-1");
}

#[test]
fn session_id_equality() {
    let a1 = SessionId::new("alice", "laptop", "conv-1");
    let a2 = SessionId::new("alice", "laptop", "conv-1");
    let b = SessionId::new("bob", "desktop", "conv-2");
    assert_eq!(a1, a2);
    assert_ne!(a1, b);
}

#[test]
fn lamport_ordering() {
    let t0 = Lamport::zero();
    let t1 = t0.next();
    let t2 = t1.next();
    assert!(t0 < t1);
    assert!(t1 < t2);
}

#[test]
fn lamport_advance_takes_max() {
    let local = Lamport(5);
    let remote = Lamport(10);
    let merged = local.advance(remote);
    assert_eq!(merged.0, 11);
}

#[test]
fn new_context_has_no_patches() {
    let snap = fib_snap(5);
    let ctx = SharedContext::new("test", snap, alice());
    assert_eq!(ctx.patch_count(), 0);
    assert_eq!(ctx.message_count(), 0);
}

#[test]
fn current_snapshot_with_no_patches_equals_base() {
    let snap = fib_snap(5);
    let ctx = SharedContext::new("test", snap.clone(), alice());
    let current = ctx.current_snapshot().unwrap();
    assert_eq!(current.regs, snap.regs);
    assert_eq!(current.pc, snap.pc);
}

#[test]
fn apply_patch_changes_current_snapshot() {
    let snap = fib_snap(5);
    let patch = reg_patch(&snap, 0, 999, "set r0=999");
    let mut ctx = SharedContext::new("test", snap.clone(), alice());
    ctx.apply_patch(alice(), "test patch", patch).unwrap();
    let current = ctx.current_snapshot().unwrap();
    assert_eq!(current.regs[0], 999);
    assert_eq!(current.regs[1], snap.regs[1]);
}

#[test]
fn apply_patch_records_patch_event() {
    let snap = fib_snap(5);
    let patch = reg_patch(&snap, 0, 999, "set r0=999");
    let mut ctx = SharedContext::new("evented", snap, alice());

    ctx.apply_patch(alice(), "test patch", patch).unwrap();
    let current = ctx.current_snapshot().unwrap();
    let last = current.event_log.entries().last().unwrap();

    assert!(matches!(
        &last.event,
        AeonEvent::PatchApplied {
            context_id,
            author,
            description,
            patch_count,
        } if context_id == "evented"
            && author == alice().display()
            && description == "test patch"
            && *patch_count == 1
    ));
    assert!(current.event_log.verify().is_ok());
}

#[test]
fn patches_are_stacked_in_order() {
    let snap = fib_snap(5);
    let p1 = reg_patch(&snap, 0, 100, "patch 1");
    let mut ctx = SharedContext::new("test", snap.clone(), alice());
    ctx.apply_patch(alice(), "p1", p1).unwrap();
    let after_p1 = ctx.current_snapshot().unwrap();
    let p2 = reg_patch(&after_p1, 1, 200, "patch 2");
    ctx.apply_patch(alice(), "p2", p2).unwrap();
    let current = ctx.current_snapshot().unwrap();
    assert_eq!(current.regs[0], 100);
    assert_eq!(current.regs[1], 200);
}

#[test]
fn patch_clock_is_monotonically_increasing() {
    let snap = fib_snap(3);
    let mut ctx = SharedContext::new("test", snap.clone(), alice());
    let p1 = reg_patch(&snap, 0, 1, "p1");
    let t1 = ctx.apply_patch(alice(), "p1", p1).unwrap();
    let snap2 = ctx.current_snapshot().unwrap();
    let p2 = reg_patch(&snap2, 1, 2, "p2");
    let t2 = ctx.apply_patch(alice(), "p2", p2).unwrap();
    assert!(t1 < t2);
}

#[test]
fn two_sessions_see_each_others_patches() {
    let snap = fib_snap(5);
    let mut ctx = SharedContext::new("collab", snap.clone(), alice());
    ctx.join(bob());
    let pa = reg_patch(&snap, 0, 42, "alice sets r0");
    ctx.apply_patch(alice(), "alice edit", pa).unwrap();
    let after_alice = ctx.current_snapshot().unwrap();
    assert_eq!(after_alice.regs[0], 42);
    let pb = reg_patch(&after_alice, 1, 99, "bob sets r1");
    ctx.apply_patch(bob(), "bob edit", pb).unwrap();
    let final_snap = ctx.current_snapshot().unwrap();
    assert_eq!(final_snap.regs[0], 42);
    assert_eq!(final_snap.regs[1], 99);
}

#[test]
fn patch_attribution_is_correct() {
    let snap = fib_snap(3);
    let mut ctx = SharedContext::new("attr", snap.clone(), alice());
    ctx.join(bob());
    let pa = reg_patch(&snap, 0, 1, "alice");
    ctx.apply_patch(alice(), "alice edit", pa).unwrap();
    let snap2 = ctx.current_snapshot().unwrap();
    let pb = reg_patch(&snap2, 1, 2, "bob");
    ctx.apply_patch(bob(), "bob edit", pb).unwrap();
    assert_eq!(ctx.patches[0].author, alice());
    assert_eq!(ctx.patches[1].author, bob());
}

#[test]
fn message_is_visible_to_all_sessions() {
    let snap = fib_snap(3);
    let mut ctx = SharedContext::new("msg", snap, alice());
    ctx.join(bob());
    let _clock = ctx.post_message(alice(), "hello from alice");
    assert_eq!(ctx.message_count(), 1);
    assert_eq!(ctx.messages[0].text, "hello from alice");
    assert_eq!(ctx.messages[0].author, alice());
}

#[test]
fn messages_and_patches_have_consistent_clocks() {
    let snap = fib_snap(3);
    let mut ctx = SharedContext::new("clocks", snap.clone(), alice());
    let t_patch = {
        let patch = reg_patch(&snap, 0, 1, "p");
        ctx.apply_patch(alice(), "p", patch).unwrap()
    };
    let t_msg = ctx.post_message(bob(), "hello");
    assert!(t_msg > t_patch);
}

#[test]
fn alice_can_undo_her_own_patch() {
    let snap = fib_snap(5);
    let original_r0 = snap.regs[0];
    let mut ctx = SharedContext::new("undo", snap.clone(), alice());
    let patch = reg_patch(&snap, 0, 999, "alice changes r0");
    ctx.apply_patch(alice(), "change", patch).unwrap();
    assert_eq!(ctx.current_snapshot().unwrap().regs[0], 999);
    let pos = ctx
        .patches
        .iter()
        .rposition(|p| p.author == alice())
        .unwrap();
    ctx.patches.remove(pos);
    assert_eq!(ctx.current_snapshot().unwrap().regs[0], original_r0);
}

#[test]
fn bob_undo_does_not_affect_alices_patches() {
    let snap = fib_snap(5);
    let mut ctx = SharedContext::new("selective-undo", snap.clone(), alice());
    ctx.join(bob());
    let pa = reg_patch(&snap, 0, 10, "alice");
    ctx.apply_patch(alice(), "alice", pa).unwrap();
    let snap2 = ctx.current_snapshot().unwrap();
    let pb = reg_patch(&snap2, 1, 20, "bob");
    ctx.apply_patch(bob(), "bob", pb).unwrap();
    let bob_pos = ctx.patches.iter().rposition(|p| p.author == bob()).unwrap();
    ctx.patches.remove(bob_pos);
    let result = ctx.current_snapshot().unwrap();
    assert_eq!(result.regs[0], 10);
    assert_eq!(result.regs[1], snap.regs[1]);
}

#[test]
fn shared_context_roundtrips_through_bytes() {
    let snap = fib_snap(5);
    let mut ctx = SharedContext::new("serial", snap.clone(), alice());
    ctx.join(bob());
    let patch = reg_patch(&snap, 0, 77, "patch");
    ctx.apply_patch(alice(), "test", patch).unwrap();
    ctx.post_message(bob(), "hi alice");
    let bytes = ctx.to_bytes();
    let ctx2 = SharedContext::from_bytes(&bytes).unwrap();
    assert_eq!(ctx2.patch_count(), 1);
    assert_eq!(ctx2.message_count(), 1);
    assert_eq!(ctx2.connected_sessions.len(), 2);
    assert_eq!(ctx2.current_snapshot().unwrap().regs[0], 77);
}

#[test]
fn registry_create_and_get() {
    let registry = ContextRegistry::new();
    let snap = fib_snap(3);
    registry.create("my-context", snap, alice());
    assert!(registry.get("my-context").is_some());
    assert!(registry.get("nonexistent").is_none());
}

#[test]
fn registry_list_all_contexts() {
    let registry = ContextRegistry::new();
    registry.create("ctx-a", fib_snap(1), alice());
    registry.create("ctx-b", fib_snap(2), bob());
    let mut ids = registry.list();
    ids.sort();
    assert!(ids.contains(&"ctx-a".to_string()));
    assert!(ids.contains(&"ctx-b".to_string()));
}

#[test]
fn resumed_snapshot_executes_correctly_with_edits() {
    let program = programs::fibonacci(10);
    let store = ProgramStore::new();
    store.add(program.clone());
    let mut state = VMState::new(&program);
    state.run_bounded(&program, 10);
    let snap = Snapshot::capture(&state);
    let mut ctx = SharedContext::new("collab-resume", snap.clone(), alice());
    ctx.join(bob());
    let patch = reg_patch(&snap, 0, 2, "alice: 2 more");
    ctx.apply_patch(alice(), "alice", patch).unwrap();
    let final_snap = ctx.current_snapshot().unwrap();
    assert_eq!(final_snap.regs[0], 2);
    let mut state2 = final_snap.restore(&store).unwrap();
    state2.run(&program).unwrap();
    assert_eq!(state2.regs[0], 0);
    assert!(state2.steps > state.steps);
}
