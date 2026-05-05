use aeon_vm::editor::PatchSet;
use aeon_vm::eventlog::AeonEvent;
use aeon_vm::program::Program;
use aeon_vm::protocol::{
    parse_program_id, read_msg, write_error, write_msg, ERROR, NEED_PROGRAM, OK, PATCHSET, PROGRAM,
    SNAPSHOT,
};
use aeon_vm::snapshot::Snapshot;
use aeon_vm::vm::VMState;
use std::env;
use std::net::TcpStream;
use std::path::Path;

fn main() {
    if let Err(err) = run() {
        eprintln!("[aeon-send] {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err(
            "usage: aeon-send <program.aeon> [--to host:port] --snap-at <n> [--patch patch.bin]"
                .into(),
        );
    }

    let program_path = Path::new(&args[1]);
    let target = flag(&args, "--to")
        .or_else(|| positional_target(&args))
        .unwrap_or_else(|| "127.0.0.1:9999".to_string());
    let snap_at = flag(&args, "--snap-at")
        .ok_or_else(|| "--snap-at is required".to_string())?
        .parse::<usize>()
        .map_err(|err| format!("invalid --snap-at: {}", err))?;

    let program = Program::load(program_path)
        .map_err(|err| format!("failed to load {}: {}", program_path.display(), err))?;
    let mut state = VMState::new(&program);
    let (steps, halted) = state.run_bounded(&program, snap_at);
    println!("[aeon-send] Ran {} steps (halted={})", steps, halted);
    if halted {
        return Ok(());
    }

    let mut snapshot = Snapshot::capture(&state);
    let patchset = load_patchset(&args)?;

    let mut stream =
        TcpStream::connect(&target).map_err(|err| format!("connect {}: {}", target, err))?;
    let from = stream
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| "sender".to_string());
    snapshot.append_event(AeonEvent::VMMigrated {
        program_id: program.id(),
        from,
        to: target.clone(),
        steps: state.steps,
    });
    save_snapshot(program_path, &snapshot)?;

    write_msg(&mut stream, SNAPSHOT, &snapshot.to_bytes()).map_err(|err| err.to_string())?;
    write_msg(&mut stream, PATCHSET, &patchset.to_bytes()).map_err(|err| err.to_string())?;

    match read_msg(&mut stream).map_err(|err| err.to_string())? {
        msg if msg.msg_type == OK => {
            println!("[aeon-send] Migration accepted by {}", target);
            Ok(())
        }
        msg if msg.msg_type == NEED_PROGRAM => {
            let requested = parse_program_id(&msg.payload).map_err(|err| err.to_string())?;
            let expected = program.id();
            if requested != expected {
                let message = "receiver requested a different ProgramId";
                let _ = write_error(&mut stream, message);
                return Err(message.into());
            }

            write_msg(&mut stream, PROGRAM, &program.to_bytes()).map_err(|err| err.to_string())?;
            let final_msg = read_msg(&mut stream).map_err(|err| err.to_string())?;
            if final_msg.msg_type == OK {
                println!(
                    "[aeon-send] Program transferred; migration accepted by {}",
                    target
                );
                Ok(())
            } else if final_msg.msg_type == ERROR {
                Err(String::from_utf8_lossy(&final_msg.payload).into_owned())
            } else {
                Err(format!("unexpected response type {}", final_msg.msg_type))
            }
        }
        msg if msg.msg_type == ERROR => Err(String::from_utf8_lossy(&msg.payload).into_owned()),
        msg => Err(format!("unexpected response type {}", msg.msg_type)),
    }
}

fn save_snapshot(program_path: &Path, snapshot: &Snapshot) -> Result<(), String> {
    let path = program_path.with_extension("snap");
    snapshot
        .save(&path)
        .map_err(|err| format!("save snapshot {}: {}", path.display(), err))?;
    println!("[aeon-send] Snapshot saved to {}", path.display());
    Ok(())
}

fn load_patchset(args: &[String]) -> Result<PatchSet, String> {
    if let Some(path) = flag(args, "--patch") {
        let bytes = std::fs::read(&path).map_err(|err| format!("read patch {}: {}", path, err))?;
        PatchSet::from_bytes(&bytes)
    } else {
        Ok(PatchSet::empty("no patch"))
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn positional_target(args: &[String]) -> Option<String> {
    args.iter()
        .skip(2)
        .find(|arg| !arg.starts_with("--") && arg.contains(':'))
        .cloned()
}
