use aeon_vm::daemon::{default_socket_path, default_state_dir, send_request, serve, DaemonRequest};
use aeon_vm::snapshot::Snapshot;
use std::env;
use std::path::Path;

fn main() {
    if let Err(err) = run() {
        eprintln!("[aeon] {}", err);
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("log") => {
            let target = args
                .get(2)
                .ok_or_else(|| "usage: aeon log <snapshot.snap|vm-id>".to_string())?;
            if Path::new(target).exists() {
                print_snapshot_log(Path::new(target))
            } else {
                print_daemon_response("log", &[target.clone()])
            }
        }
        Some("daemon") => serve(&default_socket_path(), default_state_dir()),
        Some("ps") => print_daemon_response("ps", &[]),
        Some("run") => {
            let path = args
                .get(2)
                .ok_or_else(|| "usage: aeon run <file.aeon>".to_string())?;
            print_daemon_response("run", &[path.clone()])
        }
        Some("pause") | Some("resume") | Some("share") => {
            let cmd = args[1].as_str();
            let id = args
                .get(2)
                .ok_or_else(|| format!("usage: aeon {} <id>", cmd))?;
            print_daemon_response(cmd, &[id.clone()])
        }
        Some("migrate") => {
            let id = args
                .get(2)
                .ok_or_else(|| "usage: aeon migrate <id> --to <host:port>".to_string())?;
            let to = args
                .windows(2)
                .find(|window| window[0] == "--to")
                .map(|window| window[1].clone())
                .ok_or_else(|| "usage: aeon migrate <id> --to <host:port>".to_string())?;
            print_daemon_response("migrate", &[id.clone(), to])
        }
        Some("devices") => print_daemon_response("devices", &[]),
        _ => Err("usage: aeon daemon|ps|run|pause|resume|migrate|log|devices|share".into()),
    }
}

fn print_snapshot_log(path: &Path) -> Result<(), String> {
    let snap = Snapshot::load(path).map_err(|err| format!("load {}: {}", path.display(), err))?;
    snap.event_log.verify()?;
    for line in snap.event_log.lines() {
        println!("{}", line);
    }
    Ok(())
}

fn print_daemon_response(cmd: &str, args: &[String]) -> Result<(), String> {
    let response = send_request(
        &default_socket_path(),
        &DaemonRequest {
            cmd: cmd.to_string(),
            args: args.to_vec(),
        },
    )?;
    print!("{}", response);
    Ok(())
}
