// bin/aeon-recv.rs — receive a snapshot over TCP and resume
// Usage: aeon-recv <program.aeon>   (program must already be present)

use aeon_vm::snapshot::Snapshot;
use aeon_vm::store::ProgramStore;
use std::env;
use std::io::Read;
use std::net::TcpListener;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: aeon-recv <program.aeon>");
        std::process::exit(1);
    }

    let store = ProgramStore::new();
    store.load_file(Path::new(&args[1])).unwrap_or_else(|e| {
        eprintln!("Cannot load program: {}", e);
        std::process::exit(1);
    });

    let listener = TcpListener::bind("0.0.0.0:9999").expect("bind :9999 failed");
    println!("[aeon-recv] Listening on :9999...");

    let (mut stream, peer) = listener.accept().expect("accept failed");
    println!("[aeon-recv] Connection from {}", peer);

    let mut len_bytes = [0u8; 8];
    stream.read_exact(&mut len_bytes).expect("read length");
    let len = u64::from_be_bytes(len_bytes) as usize;

    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).expect("read snapshot");

    let snap = Snapshot::from_bytes(&buf).expect("deserialize snapshot");
    println!("[aeon-recv] Snapshot received: pc={} steps_so_far={}", snap.pc, snap.steps);

    let mut state = snap.restore(&store).unwrap_or_else(|e| {
        eprintln!("Restore failed: {}", e);
        std::process::exit(1);
    });

    let program = store.get(&state.program_id()).unwrap();
    let steps = state.run(&program).expect("run failed");
    println!("[aeon-recv] Completed. Total steps across both machines: {}", steps);

    let nonzero: Vec<_> = state.regs.iter().enumerate()
        .filter(|(_, &v)| v != 0).map(|(i, &v)| (i, v)).collect();
    for (i, v) in nonzero {
        println!("  r{} = {}", i, v);
    }
}
