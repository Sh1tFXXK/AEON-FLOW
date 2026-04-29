use aeon_vm::console::{ConsoleResult, FlowConsole};
use aeon_vm::editor::PatchSet;
use aeon_vm::program::Program;
use aeon_vm::protocol::{
    read_msg, write_error, write_msg, NEED_PROGRAM, OK, PATCHSET, PROGRAM, SNAPSHOT,
};
use aeon_vm::session::SessionId;
use aeon_vm::snapshot::Snapshot;
use aeon_vm::store::ProgramStore;
use std::env;
use std::net::{TcpListener, TcpStream};
use std::path::Path;

fn main() {
    if let Err(err) = run() {
        eprintln!("[aeon-recv] {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let bind = flag(&args, "--bind").unwrap_or_else(|| "0.0.0.0:9999".to_string());
    let session = flag(&args, "--session")
        .map(|value| SessionId::from_str(&value))
        .unwrap_or_else(|| SessionId::new("receiver", "local", "console"));

    let store = ProgramStore::new();
    if let Some(program_path) = flag(&args, "--program") {
        store
            .load_file(Path::new(&program_path))
            .map_err(|err| format!("cannot load program {}: {}", program_path, err))?;
    }

    let listener = TcpListener::bind(&bind).map_err(|err| format!("bind {}: {}", bind, err))?;
    println!("[aeon-recv] Listening on {}...", bind);

    let (mut stream, peer) = listener.accept().map_err(|err| err.to_string())?;
    println!("[aeon-recv] Connection from {}", peer);

    let snap = receive_snapshot(&mut stream, &store)?;
    write_msg(&mut stream, OK, &[]).map_err(|err| err.to_string())?;

    let console = FlowConsole::new(session, snap);
    match console.run(&store) {
        ConsoleResult::Resume(snapshot) | ConsoleResult::Discard(snapshot) => {
            resume(snapshot, &store)?;
        }
        ConsoleResult::Quit => println!("[aeon-recv] Console exited without resuming"),
    }

    Ok(())
}

fn receive_snapshot(stream: &mut TcpStream, store: &ProgramStore) -> Result<Snapshot, String> {
    let snap_msg = read_msg(stream).map_err(|err| err.to_string())?;
    if snap_msg.msg_type != SNAPSHOT {
        return protocol_fail(stream, "expected SNAPSHOT as first message");
    }

    let mut snap = Snapshot::from_bytes(&snap_msg.payload)
        .map_err(|err| format!("invalid snapshot: {}", err))?;
    println!(
        "[aeon-recv] Snapshot received: pc={} steps_so_far={}",
        snap.pc, snap.steps
    );

    let patch_msg = read_msg(stream).map_err(|err| err.to_string())?;
    if patch_msg.msg_type != PATCHSET {
        return protocol_fail(stream, "expected PATCHSET after SNAPSHOT");
    }

    let patchset = PatchSet::from_bytes(&patch_msg.payload)
        .map_err(|err| format!("invalid patchset: {}", err))?;
    snap = patchset.apply(&snap)?;

    if !store.has(&snap.program_id()) {
        write_msg(stream, NEED_PROGRAM, &snap.program_id()).map_err(|err| err.to_string())?;
        let prog_msg = read_msg(stream).map_err(|err| err.to_string())?;
        if prog_msg.msg_type != PROGRAM {
            return protocol_fail(stream, "expected PROGRAM after NEED_PROGRAM");
        }

        let program = Program::from_bytes(&prog_msg.payload)
            .map_err(|err| format!("invalid program: {}", err))?;
        if program.id() != snap.program_id() {
            return protocol_fail(stream, "received PROGRAM with mismatched ProgramId");
        }
        store.add(program);
    }

    Ok(snap)
}

fn resume(snapshot: Snapshot, store: &ProgramStore) -> Result<(), String> {
    let mut state = snapshot.restore(store)?;
    let program = store
        .get(&state.program_id())
        .ok_or_else(|| "program missing after restore".to_string())?;
    let steps = state.run(&program).map_err(|err| err.to_string())?;

    println!(
        "[aeon-recv] Completed. Total steps across both machines: {}",
        steps
    );
    for (idx, value) in state
        .regs
        .iter()
        .enumerate()
        .filter(|(_, value)| **value != 0)
    {
        println!("  r{} = {}", idx, value);
    }

    Ok(())
}

fn protocol_fail<T>(stream: &mut TcpStream, message: &str) -> Result<T, String> {
    let _ = write_error(stream, message);
    Err(message.to_string())
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}
