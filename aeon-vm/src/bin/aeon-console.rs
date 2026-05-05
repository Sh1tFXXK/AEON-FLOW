use aeon_vm::console::{ConsoleResult, FlowConsole};
use aeon_vm::session::SessionId;
use aeon_vm::snapshot::Snapshot;
use aeon_vm::store::ProgramStore;
use std::env;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(err) = run() {
        eprintln!("[aeon-console] {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    let snapshot_path = flag(&args, "--load")
        .ok_or_else(|| "usage: aeon-console --load <snapshot.snap> [--program <program.aeon>] [--session <user@device/conv>]".to_string())?;
    let session = flag(&args, "--session")
        .map(|value| SessionId::from_str(&value))
        .unwrap_or_else(|| SessionId::new("user", "local", "console"));

    let snapshot_path = PathBuf::from(snapshot_path);
    let snapshot = Snapshot::load(&snapshot_path)
        .map_err(|err| format!("load {}: {}", snapshot_path.display(), err))?;
    let store = ProgramStore::new();
    if let Some(path) = program_path(&args, &snapshot_path) {
        store
            .load_file(&path)
            .map_err(|err| format!("load program {}: {}", path.display(), err))?;
    }

    let console = FlowConsole::new(session, snapshot);
    match console.run(&store) {
        ConsoleResult::Resume(snapshot) | ConsoleResult::Discard(snapshot) => {
            resume(snapshot, &store)
        }
        ConsoleResult::Quit => {
            println!("[aeon-console] Console exited without resuming");
            Ok(())
        }
    }
}

fn resume(snapshot: Snapshot, store: &ProgramStore) -> Result<(), String> {
    if !store.has(&snapshot.program_id()) {
        return Err(
            "program missing; pass --program or keep <snapshot-stem>.aeon next to the snapshot"
                .into(),
        );
    }

    let mut state = snapshot.restore(store)?;
    let program = store
        .get(&state.program_id())
        .ok_or_else(|| "program missing after restore".to_string())?;
    let steps = state.run(&program).map_err(|err| err.to_string())?;

    println!("[aeon-console] Completed. Total steps: {}", steps);
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

fn program_path(args: &[String], snapshot_path: &Path) -> Option<PathBuf> {
    flag(args, "--program").map(PathBuf::from).or_else(|| {
        snapshot_path
            .with_extension("aeon")
            .exists()
            .then(|| snapshot_path.with_extension("aeon"))
    })
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}
