use crate::eventlog::AeonEvent;
use crate::program::Program;
use crate::snapshot::Snapshot;
use crate::store::ProgramStore;
use crate::vm::VMState;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
#[cfg(unix)]
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VMInfo {
    pub id: String,
    pub status: String,
    pub program_path: String,
    pub program_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VMRecord {
    id: String,
    status: String,
    program_path: String,
    program_id: String,
    snapshot_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    next_id: u64,
    vms: HashMap<String, VMRecord>,
}

#[derive(Debug)]
pub struct DaemonState {
    state_dir: PathBuf,
    next_id: u64,
    vms: HashMap<String, VMRecord>,
    pub event_tx: tokio::sync::broadcast::Sender<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DaemonRequest {
    pub cmd: String,
    #[serde(default)]
    pub args: Vec<String>,
}

impl DaemonState {
    pub fn new(state_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&state_dir).map_err(|err| err.to_string())?;
        let (tx, _) = tokio::sync::broadcast::channel(100);
        Ok(DaemonState {
            state_dir,
            next_id: 1,
            vms: HashMap::new(),
            event_tx: tx,
        })
    }

    pub fn recover(state_dir: PathBuf) -> Result<Self, String> {
        let manifest_path = state_dir.join("daemon_state.json");
        if !manifest_path.exists() {
            return Self::new(state_dir);
        }
        let bytes = std::fs::read(&manifest_path)
            .map_err(|err| format!("read {}: {}", manifest_path.display(), err))?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|err| err.to_string())?;
        let state = DaemonState {
            state_dir,
            next_id: manifest.next_id,
            vms: manifest.vms,
            event_tx: tokio::sync::broadcast::channel(100).0,
        };
        state.append_daemon_restart_events()?;
        Ok(state)
    }

    fn emit(&self, event: &str) {
        let _ = self.event_tx.send(event.to_string());
    }

    pub fn run_program(&mut self, program_path: &Path) -> Result<String, String> {
        let program = Program::load(program_path)
            .map_err(|err| format!("load {}: {}", program_path.display(), err))?;
        let state = VMState::new(&program);
        let snapshot = Snapshot::capture(&state);
        let id = format!("vm-{}", self.next_id);
        self.next_id += 1;
        let snapshot_path = self.state_dir.join(format!("{}.snap", id));
        snapshot
            .save(&snapshot_path)
            .map_err(|err| format!("save snapshot: {}", err))?;
        self.vms.insert(
            id.clone(),
            VMRecord {
                id: id.clone(),
                status: "paused".into(),
                program_path: program_path.display().to_string(),
                program_id: hex_id(&program.id()),
                snapshot_path: snapshot_path.display().to_string(),
            },
        );
        self.save_manifest()?;
        self.emit(&format!(r#"{{"type": "VM_CREATED", "id": "{}"}}"#, id));
        Ok(id)
    }

    pub fn ps(&self) -> Vec<VMInfo> {
        let mut out = self
            .vms
            .values()
            .map(|record| VMInfo {
                id: record.id.clone(),
                status: record.status.clone(),
                program_path: record.program_path.clone(),
                program_id: record.program_id.clone(),
            })
            .collect::<Vec<_>>();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub fn pause(&mut self, id: &str) -> Result<(), String> {
        let record = self
            .vms
            .get_mut(id)
            .ok_or_else(|| format!("unknown VM {}", id))?;
        record.status = "paused".into();
        self.save_manifest()
    }

    pub fn resume(&mut self, id: &str) -> Result<(), String> {
        let record = self
            .vms
            .get_mut(id)
            .ok_or_else(|| format!("unknown VM {}", id))?;
        let program = Program::load(Path::new(&record.program_path))
            .map_err(|err| format!("load program: {}", err))?;
        let store = ProgramStore::new();
        store.add(program.clone());
        let snap = Snapshot::load(Path::new(&record.snapshot_path))
            .map_err(|err| format!("load snapshot: {}", err))?;
        let mut state = snap.restore(&store)?;
        state.run(&program).map_err(|err| err.to_string())?;
        let final_snap = Snapshot::capture(&state);
        final_snap
            .save(Path::new(&record.snapshot_path))
            .map_err(|err| format!("save snapshot: {}", err))?;
        record.status = "completed".into();
        self.save_manifest()
    }

    pub fn migrate(&mut self, id: &str, to: &str) -> Result<(), String> {
        let record = self
            .vms
            .get_mut(id)
            .ok_or_else(|| format!("unknown VM {}", id))?;
        let mut snap = Snapshot::load(Path::new(&record.snapshot_path))
            .map_err(|err| format!("load snapshot: {}", err))?;
        snap.append_event(AeonEvent::VMMigrated {
            program_id: snap.program_id(),
            from: "daemon".into(),
            to: to.to_string(),
            steps: snap.steps,
        });
        snap.save(Path::new(&record.snapshot_path))
            .map_err(|err| format!("save snapshot: {}", err))?;
        record.status = "migrated".into();
        self.save_manifest()?;
        self.emit(&format!(r#"{{"type": "VM_MIGRATED", "id": "{}", "to": "{}"}}"#, id, to));
        Ok(())
    }

    pub fn log(&self, id: &str) -> Result<Vec<String>, String> {
        let record = self
            .vms
            .get(id)
            .ok_or_else(|| format!("unknown VM {}", id))?;
        let snap = Snapshot::load(Path::new(&record.snapshot_path))
            .map_err(|err| format!("load snapshot: {}", err))?;
        snap.event_log.verify()?;
        Ok(snap.event_log.lines())
    }

    pub fn share(&self, id: &str) -> Result<String, String> {
        if self.vms.contains_key(id) {
            Ok(format!("ctx-{}", id))
        } else {
            Err(format!("unknown VM {}", id))
        }
    }

    fn save_manifest(&self) -> Result<(), String> {
        let manifest = Manifest {
            next_id: self.next_id,
            vms: self.vms.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
        std::fs::write(self.state_dir.join("daemon_state.json"), bytes)
            .map_err(|err| err.to_string())
    }

    fn append_daemon_restart_events(&self) -> Result<(), String> {
        for record in self.vms.values() {
            let path = Path::new(&record.snapshot_path);
            let mut snap = Snapshot::load(path)
                .map_err(|err| format!("load snapshot {}: {}", record.snapshot_path, err))?;
            snap.append_event(AeonEvent::DaemonRestart {
                vm_id: record.id.clone(),
            });
            snap.save(path)
                .map_err(|err| format!("save snapshot {}: {}", record.snapshot_path, err))?;
        }
        Ok(())
    }
}

pub fn handle_json_request(state: &mut DaemonState, input: &str) -> Result<String, String> {
    let request: DaemonRequest = serde_json::from_str(input).map_err(|err| err.to_string())?;
    let response = match request.cmd.as_str() {
        "ps" => json!({"ok": true, "vms": state.ps()}),
        "run" => {
            let path = required_arg(&request, 0)?;
            let id = state.run_program(Path::new(path))?;
            json!({"ok": true, "id": id})
        }
        "pause" => {
            state.pause(required_arg(&request, 0)?)?;
            json!({"ok": true})
        }
        "resume" => {
            state.resume(required_arg(&request, 0)?)?;
            json!({"ok": true})
        }
        "migrate" => {
            state.migrate(required_arg(&request, 0)?, required_arg(&request, 1)?)?;
            json!({"ok": true})
        }
        "log" => json!({"ok": true, "events": state.log(required_arg(&request, 0)?)?}),
        "devices" => json!({"ok": true, "devices": ["local"]}),
        "share" => json!({"ok": true, "context": state.share(required_arg(&request, 0)?)?}),
        other => json!({"ok": false, "error": format!("unknown command {}", other)}),
    };
    serde_json::to_string(&response).map_err(|err| err.to_string())
}

pub fn default_socket_path() -> PathBuf {
    std::env::var("AEON_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/aeon.sock"))
}

pub fn default_state_dir() -> PathBuf {
    std::env::var("AEON_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/aeon-state"))
}

#[cfg(unix)]
pub async fn serve(socket_path: &Path, state_dir: PathBuf) -> Result<(), String> {
    use std::os::unix::net::UnixListener;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;
    use futures_util::StreamExt;

    if socket_path.exists() {
        std::fs::remove_file(socket_path).map_err(|err| err.to_string())?;
    }

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(
        DaemonState::recover(state_dir).map_err(|err| err.to_string())?,
    ));

    // Command listener
    let listener = UnixListener::bind(socket_path).map_err(|err| err.to_string())?;
    listener.set_nonblocking(true).unwrap();
    let state_clone = state.clone();
    tokio::task::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept() {
                let mut state = state_clone.lock().await;
                let mut stream = stream;
                let mut line = String::new();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let _ = reader.read_line(&mut line);
                let response = handle_json_request(&mut state, line.trim())
                    .unwrap_or_else(|err| json!({"ok": false, "error": err}).to_string());
                let _ = writeln!(stream, "{}", response);
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }
    });

    // WebSocket broadcaster
    let ws_listener = TcpListener::bind("127.0.0.1:8080").await.map_err(|e| e.to_string())?;
    while let Ok((stream, _)) = ws_listener.accept().await {
        let tx = state.lock().await.event_tx.clone();
        let mut rx = tx.subscribe();
        tokio::task::spawn(async move {
            use futures_util::SinkExt;
            if let Ok(ws_stream) = accept_async(stream).await {
                let (mut write, _) = ws_stream.split();
                while let Ok(msg) = rx.recv().await {
                    let _ = write.send(tokio_tungstenite::tungstenite::Message::Text(msg)).await;
                }
            }
        });
    }
    Ok(())
}

#[cfg(unix)]
pub fn send_request(socket_path: &Path, request: &DaemonRequest) -> Result<String, String> {
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path).map_err(|err| err.to_string())?;
    let payload = serde_json::to_string(request).map_err(|err| err.to_string())?;
    writeln!(stream, "{}", payload).map_err(|err| err.to_string())?;
    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|err| err.to_string())?;
    Ok(response)
}

#[cfg(not(unix))]
pub async fn serve(_socket_path: &Path, _state_dir: PathBuf) -> Result<(), String> {
    Err("aeon-daemon currently requires Unix sockets".into())
}

#[cfg(not(unix))]
pub fn send_request(_socket_path: &Path, _request: &DaemonRequest) -> Result<String, String> {
    Err("aeon CLI currently requires Unix sockets".into())
}

fn required_arg(request: &DaemonRequest, index: usize) -> Result<&str, String> {
    request
        .args
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| format!("{} missing argument {}", request.cmd, index))
}

fn hex_id(id: &[u8; 32]) -> String {
    id.iter().map(|byte| format!("{:02x}", byte)).collect()
}

#[cfg(test)]
mod tests {
    #[cfg(not(unix))]
    #[tokio::test]
    async fn non_unix_serve_matches_async_api() {
        let err = super::serve(
            &super::default_socket_path(),
            super::default_state_dir(),
        )
        .await
        .expect_err("non-Unix daemon socket support should be unavailable");

        assert!(err.contains("requires Unix sockets"));
    }
}
