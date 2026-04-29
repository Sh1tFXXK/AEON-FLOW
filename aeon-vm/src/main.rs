// src/main.rs — aeon-run
// Usage:
//   aeon-run <file.aeon>                    run to completion
//   aeon-run <file.aeon> --snap-at <n>      snapshot after n steps, save to file.snap
//   aeon-run <file.aeon> --restore <snap>   restore snapshot and continue
//
use aeon_vm::snapshot::Snapshot;
use aeon_vm::store::ProgramStore;
use aeon_vm::vm::VMState;
use std::env;
use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: aeon-run <file.aeon> [--snap-at <n>] [--restore <snap>]");
        std::process::exit(1);
    }

    let program_path = Path::new(&args[1]);
    let store = ProgramStore::new();
    let program_id = store.load_file(program_path).unwrap_or_else(|e| {
        eprintln!("Failed to load {}: {}", program_path.display(), e);
        std::process::exit(1);
    });
    let program = store.get(&program_id).unwrap();

    // Parse flags
    let snap_at: Option<usize> = args
        .windows(2)
        .find(|w| w[0] == "--snap-at")
        .and_then(|w| w[1].parse().ok());

    let restore_path: Option<&str> = args
        .windows(2)
        .find(|w| w[0] == "--restore")
        .map(|w| w[1].as_str());

    // Build initial state
    let mut state = if let Some(snap_file) = restore_path {
        let snap = Snapshot::load(Path::new(snap_file)).unwrap_or_else(|e| {
            eprintln!("Failed to load snapshot: {}", e);
            std::process::exit(1);
        });
        println!(
            "[aeon-run] Restored snapshot (pc={}, steps={})",
            snap.pc, snap.steps
        );
        snap.restore(&store).unwrap_or_else(|e| {
            eprintln!("Restore failed: {}", e);
            std::process::exit(1);
        })
    } else {
        VMState::new(&program)
    };

    // Run
    if let Some(n) = snap_at {
        let (steps, halted) = state.run_bounded(&program, n);
        println!("[aeon-run] Ran {} steps (halted={})", steps, halted);

        if !halted {
            let snap_path = program_path.with_extension("snap");
            let snap = Snapshot::capture(&state);
            println!(
                "[aeon-run] Snapshot: {} bytes → {}",
                snap.byte_size(),
                snap_path.display()
            );
            snap.save(&snap_path).expect("save snapshot");
        }
    } else {
        let steps = state.run(&program).unwrap_or_else(|e| {
            eprintln!("VM error: {}", e);
            std::process::exit(1);
        });
        println!("[aeon-run] Completed in {} steps", steps);
    }

    // Print non-zero registers for observability
    let nonzero: Vec<(usize, u64)> = state
        .regs
        .iter()
        .enumerate()
        .filter(|(_, &v)| v != 0)
        .map(|(i, &v)| (i, v))
        .collect();

    if !nonzero.is_empty() {
        println!("[aeon-run] Non-zero registers:");
        for (i, v) in nonzero {
            println!("  r{} = {}", i, v);
        }
    }
}
