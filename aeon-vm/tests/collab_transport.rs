use std::net::TcpListener;
use std::thread;

use aeon_vm::collab_transport::{exchange_context, serve_context_once};
use aeon_vm::editor::{PatchSet, SnapshotEditor};
use aeon_vm::program::programs;
use aeon_vm::session::{SessionId, SharedContext};
use aeon_vm::snapshot::Snapshot;
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
fn tcp_context_exchange_merges_both_peers() {
    let snap = fib_snap(5);
    let mut server_context = SharedContext::new("collab", snap.clone(), alice());
    let server_patch = reg_patch(&snap, 0, 11, "server");
    server_context
        .apply_patch(alice(), "server edit", server_patch)
        .unwrap();

    let mut client_context = SharedContext::new("collab", snap.clone(), bob());
    let client_patch = reg_patch(&snap, 1, 22, "client");
    client_context
        .apply_patch(bob(), "client edit", client_patch)
        .unwrap();

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let report = serve_context_once(listener, &mut server_context).unwrap();
        (server_context, report)
    });

    let client_report = exchange_context(&addr.to_string(), &mut client_context).unwrap();
    let (server_context, server_report) = server.join().unwrap();

    assert_eq!(client_report.patches, 1);
    assert_eq!(server_report.patches, 1);
    assert_eq!(client_context.patch_count(), 2);
    assert_eq!(server_context.patch_count(), 2);
    assert_eq!(client_context.current_snapshot().unwrap().regs[0], 11);
    assert_eq!(client_context.current_snapshot().unwrap().regs[1], 22);
    assert_eq!(server_context.current_snapshot().unwrap().regs[0], 11);
    assert_eq!(server_context.current_snapshot().unwrap().regs[1], 22);
}
