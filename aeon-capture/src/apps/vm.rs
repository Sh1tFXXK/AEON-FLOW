use super::AppCapture;
use crate::capture::{CaptureEntry, CaptureKind, CaptureSource};
use crate::engine::CaptureEngine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

pub const VM_SNAPSHOT_MIME: &str = "application/x-aeon-snapshot";
const SNAPSHOT_FORMAT_VERSION: u32 = 2;
const CAPTURE_WRAPPER_PROGRAM: &str = "capture-wrapper.aeon";

pub struct AeonVmCapture {
    pub vm_id: String,
    pub state_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AeonVmInfo {
    pub id: String,
    pub status: String,
    pub program_path: String,
    pub program_id: String,
    pub snapshot_path: Option<String>,
    pub snapshot_size: Option<usize>,
    pub format_version: Option<u32>,
    pub pc: Option<usize>,
    pub steps: Option<usize>,
    pub event_count: Option<usize>,
    pub last_event: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Manifest {
    next_id: u64,
    vms: HashMap<String, VMRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VMRecord {
    id: String,
    status: String,
    program_path: String,
    program_id: String,
    snapshot_path: String,
}

#[derive(Debug)]
struct SnapshotExport {
    record: VMRecord,
    snapshot_path: PathBuf,
    bytes: Vec<u8>,
    probe: Option<SnapshotProbe>,
    probe_error: Option<String>,
}

impl AppCapture for AeonVmCapture {
    fn app_name(&self) -> &str {
        "aeon-vm"
    }

    fn is_running(&self) -> bool {
        let state_dir = self.state_dir.clone().unwrap_or_else(default_state_dir);
        read_manifest(&state_dir)
            .map(|manifest| manifest.vms.contains_key(&self.vm_id))
            .unwrap_or(false)
    }

    fn capture(&self) -> Option<CaptureEntry> {
        let state_dir = self.state_dir.clone().unwrap_or_else(default_state_dir);
        capture_vm_snapshot_from_state_dir(&self.vm_id, state_dir).ok()
    }

    fn watch(&self, _engine: Arc<CaptureEngine>) {}
}

pub fn list_vms() -> Vec<AeonVmInfo> {
    list_vms_in_state_dir(default_state_dir()).unwrap_or_default()
}

pub fn list_recent_vms(limit: usize) -> Vec<AeonVmInfo> {
    list_recent_vms_in_state_dir(default_state_dir(), limit).unwrap_or_default()
}

pub fn list_vms_in_state_dir(state_dir: PathBuf) -> Result<Vec<AeonVmInfo>, String> {
    list_vms_in_state_dir_with_limit(state_dir, None)
}

pub fn list_recent_vms_in_state_dir(
    state_dir: PathBuf,
    limit: usize,
) -> Result<Vec<AeonVmInfo>, String> {
    list_vms_in_state_dir_with_limit(state_dir, Some(limit))
}

fn list_vms_in_state_dir_with_limit(
    state_dir: PathBuf,
    limit: Option<usize>,
) -> Result<Vec<AeonVmInfo>, String> {
    let manifest = read_manifest(&state_dir)?;
    let mut records = manifest.vms.into_values().collect::<Vec<_>>();
    records.sort_by(|a, b| compare_vm_ids_newest_first(&a.id, &b.id));
    if let Some(limit) = limit {
        records.truncate(limit);
    }
    let vms = records
        .into_iter()
        .map(|record| match export_snapshot_record(record.clone()) {
            Ok(export) => vm_info_from_export(export),
            Err(error) => vm_info_with_error(record, error),
        })
        .collect::<Vec<_>>();
    Ok(vms)
}

pub fn capture_vm_snapshot(vm_id: &str) -> Result<CaptureEntry, String> {
    capture_vm_snapshot_from_state_dir(vm_id, default_state_dir())
}

pub fn set_vm_status(vm_id: &str, status: &str) -> Result<AeonVmInfo, String> {
    set_vm_status_in_state_dir(vm_id, status, default_state_dir())
}

pub fn set_vm_status_in_state_dir(
    vm_id: &str,
    status: &str,
    state_dir: PathBuf,
) -> Result<AeonVmInfo, String> {
    let _guard = manifest_lock()
        .lock()
        .map_err(|_| "aeon-vm manifest lock poisoned".to_string())?;
    let mut manifest = read_manifest(&state_dir)?;
    let record = manifest
        .vms
        .get_mut(vm_id)
        .ok_or_else(|| format!("unknown VM {vm_id}"))?;
    record.status = status.to_string();
    let record = record.clone();
    save_manifest(&state_dir, &manifest)?;
    match export_snapshot_record(record.clone()) {
        Ok(export) => Ok(vm_info_from_export(export)),
        Err(error) => Ok(vm_info_with_error(record, error)),
    }
}

pub fn capture_vm_snapshot_from_state_dir(
    vm_id: &str,
    state_dir: PathBuf,
) -> Result<CaptureEntry, String> {
    let manifest = read_manifest(&state_dir)?;
    let record = manifest
        .vms
        .get(vm_id)
        .cloned()
        .ok_or_else(|| format!("unknown VM {vm_id}"))?;
    let export = export_snapshot_record(record)?;
    Ok(capture_entry_from_export(export))
}

pub fn auto_wrap_capture_entry(entry: &mut CaptureEntry) -> Result<Option<AeonVmInfo>, String> {
    if !auto_wrap_enabled() {
        return Ok(None);
    }
    wrap_capture_entry_as_vm_snapshot(entry)
}

pub fn wrap_capture_entry_as_vm_snapshot(
    entry: &mut CaptureEntry,
) -> Result<Option<AeonVmInfo>, String> {
    wrap_capture_entry_as_vm_snapshot_in_state_dir(entry, default_state_dir())
}

pub fn wrap_capture_entry_as_vm_snapshot_in_state_dir(
    entry: &mut CaptureEntry,
    state_dir: PathBuf,
) -> Result<Option<AeonVmInfo>, String> {
    if entry.kind == CaptureKind::VmSnapshot {
        return Ok(None);
    }

    let _guard = manifest_lock()
        .lock()
        .map_err(|_| "aeon-vm manifest lock poisoned".to_string())?;
    std::fs::create_dir_all(&state_dir)
        .map_err(|err| format!("create {}: {}", state_dir.display(), err))?;

    let program_path = ensure_capture_wrapper_program(&state_dir)?;
    let program_id = capture_wrapper_program_id()?;
    let program_id_hex = hex_id(&program_id);
    let mut manifest = read_or_new_manifest(&state_dir)?;
    let vm_id = reserve_vm_id(&mut manifest);
    let snapshot_path = state_dir.join(format!("{vm_id}.snap"));
    let program_path_string = program_path.display().to_string();
    let snapshot_path_string = snapshot_path.display().to_string();
    let state_dir_string = state_dir.display().to_string();

    entry
        .meta
        .extra
        .insert("aeon_vm_wrapped".to_string(), "true".to_string());
    entry
        .meta
        .extra
        .insert("aeon_vm_id".to_string(), vm_id.clone());
    entry.meta.extra.insert(
        "aeon_vm_snapshot_path".to_string(),
        snapshot_path_string.clone(),
    );
    entry.meta.extra.insert(
        "aeon_vm_program_path".to_string(),
        program_path_string.clone(),
    );
    entry
        .meta
        .extra
        .insert("aeon_vm_program_id".to_string(), program_id_hex.clone());
    entry
        .meta
        .extra
        .insert("aeon_vm_state_dir".to_string(), state_dir_string);
    entry.meta.extra.insert(
        "aeon_vm_snapshot_mime".to_string(),
        VM_SNAPSHOT_MIME.to_string(),
    );

    let snapshot = snapshot_for_capture(entry, program_id)?;
    let snapshot_bytes = snapshot_to_bytes(&snapshot)?;
    std::fs::write(&snapshot_path, &snapshot_bytes)
        .map_err(|err| format!("write snapshot {}: {}", snapshot_path.display(), err))?;

    let record = VMRecord {
        id: vm_id.clone(),
        status: "paused".to_string(),
        program_path: program_path_string,
        program_id: program_id_hex.clone(),
        snapshot_path: snapshot_path_string,
    };
    manifest.vms.insert(vm_id.clone(), record.clone());
    save_manifest(&state_dir, &manifest)?;

    Ok(Some(AeonVmInfo {
        id: vm_id,
        status: record.status,
        program_path: record.program_path,
        program_id: program_id_hex,
        snapshot_path: Some(record.snapshot_path),
        snapshot_size: Some(snapshot_bytes.len()),
        format_version: Some(snapshot.format_version),
        pc: Some(snapshot.pc),
        steps: Some(snapshot.steps),
        event_count: Some(snapshot.event_count()),
        last_event: snapshot.last_event(),
        error: None,
    }))
}

fn capture_entry_from_export(export: SnapshotExport) -> CaptureEntry {
    let snapshot_size = export.bytes.len();
    let title = format!(
        "AEON VM snapshot {} ({})",
        export.record.id, export.record.status
    );
    let summary = match &export.probe {
        Some(probe) => format!(
            "{} - pc {} - {} steps - {} events",
            export.record.program_path,
            probe.pc,
            probe.steps,
            probe.event_count()
        ),
        None => format!("{} - {} bytes", export.record.program_path, snapshot_size),
    };

    let mut entry = CaptureEntry::new(
        export.bytes,
        CaptureKind::VmSnapshot,
        CaptureSource::AppApi {
            app: "aeon-vm".to_string(),
        },
    )
    .with_title(&title)
    .with_summary(&summary)
    .with_app("aeon-vm");

    entry.meta.file_path = Some(export.snapshot_path.display().to_string());
    entry
        .meta
        .extra
        .insert("vm_id".to_string(), export.record.id);
    entry
        .meta
        .extra
        .insert("vm_status".to_string(), export.record.status);
    entry
        .meta
        .extra
        .insert("program_path".to_string(), export.record.program_path);
    entry
        .meta
        .extra
        .insert("program_id".to_string(), export.record.program_id);
    entry
        .meta
        .extra
        .insert("snapshot_mime".to_string(), VM_SNAPSHOT_MIME.to_string());
    entry
        .meta
        .extra
        .insert("snapshot_size".to_string(), snapshot_size.to_string());

    if let Some(probe) = export.probe {
        insert_probe_meta(&mut entry, &probe);
    }
    if let Some(error) = export.probe_error {
        entry
            .meta
            .extra
            .insert("snapshot_probe_error".to_string(), error);
    }
    entry
}

fn vm_info_from_export(export: SnapshotExport) -> AeonVmInfo {
    let probe = export.probe.as_ref();
    AeonVmInfo {
        id: export.record.id,
        status: export.record.status,
        program_path: export.record.program_path,
        program_id: export.record.program_id,
        snapshot_path: Some(export.snapshot_path.display().to_string()),
        snapshot_size: Some(export.bytes.len()),
        format_version: probe.map(|probe| probe.format_version),
        pc: probe.map(|probe| probe.pc),
        steps: probe.map(|probe| probe.steps),
        event_count: probe.map(SnapshotProbe::event_count),
        last_event: probe.and_then(SnapshotProbe::last_event),
        error: export.probe_error,
    }
}

fn vm_info_with_error(record: VMRecord, error: String) -> AeonVmInfo {
    AeonVmInfo {
        id: record.id,
        status: record.status,
        program_path: record.program_path,
        program_id: record.program_id,
        snapshot_path: Some(record.snapshot_path),
        snapshot_size: None,
        format_version: None,
        pc: None,
        steps: None,
        event_count: None,
        last_event: None,
        error: Some(error),
    }
}

fn insert_probe_meta(entry: &mut CaptureEntry, probe: &SnapshotProbe) {
    entry.meta.extra.insert(
        "format_version".to_string(),
        probe.format_version.to_string(),
    );
    entry
        .meta
        .extra
        .insert("pc".to_string(), probe.pc.to_string());
    entry
        .meta
        .extra
        .insert("steps".to_string(), probe.steps.to_string());
    entry
        .meta
        .extra
        .insert("event_count".to_string(), probe.event_count().to_string());
    if let Some(last_event) = probe.last_event() {
        entry
            .meta
            .extra
            .insert("last_event".to_string(), last_event);
    }
}

fn export_snapshot_record(record: VMRecord) -> Result<SnapshotExport, String> {
    let snapshot_path = PathBuf::from(&record.snapshot_path);
    let bytes = std::fs::read(&snapshot_path)
        .map_err(|err| format!("read snapshot {}: {}", snapshot_path.display(), err))?;
    verify_snapshot_bytes(&bytes)?;
    let (probe, probe_error) = match decode_snapshot_probe(&bytes) {
        Ok(probe) => (Some(probe), None),
        Err(error) => (None, Some(error)),
    };
    Ok(SnapshotExport {
        record,
        snapshot_path,
        bytes,
        probe,
        probe_error,
    })
}

fn read_manifest(state_dir: &Path) -> Result<Manifest, String> {
    let manifest_path = state_dir.join("daemon_state.json");
    let bytes = std::fs::read(&manifest_path)
        .map_err(|err| format!("read {}: {}", manifest_path.display(), err))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("parse manifest: {err}"))
}

fn verify_snapshot_bytes(bytes: &[u8]) -> Result<&[u8], String> {
    if bytes.len() < 32 {
        return Err("snapshot missing checksum".to_string());
    }
    let (hash_bytes, payload) = bytes.split_at(32);
    if hash_bytes != blake3::hash(payload).as_bytes() {
        return Err("snapshot checksum mismatch".to_string());
    }
    Ok(payload)
}

fn decode_snapshot_probe(bytes: &[u8]) -> Result<SnapshotProbe, String> {
    let payload = verify_snapshot_bytes(bytes)?;
    bincode::deserialize(payload).map_err(|err| format!("decode snapshot metadata: {err}"))
}

fn default_state_dir() -> PathBuf {
    std::env::var("AEON_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp/aeon-state"))
}

fn auto_wrap_enabled() -> bool {
    #[cfg(test)]
    {
        std::env::var_os("AEON_CAPTURE_ENABLE_VM_WRAP_IN_TESTS").is_some()
    }

    #[cfg(not(test))]
    {
        std::env::var("AEON_CAPTURE_DISABLE_VM_WRAP")
            .map(|value| value != "1" && value.to_lowercase() != "true")
            .unwrap_or(true)
    }
}

fn manifest_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read_or_new_manifest(state_dir: &Path) -> Result<Manifest, String> {
    let manifest_path = state_dir.join("daemon_state.json");
    if !manifest_path.exists() {
        return Ok(Manifest {
            next_id: 1,
            vms: HashMap::new(),
        });
    }
    let bytes = std::fs::read(&manifest_path)
        .map_err(|err| format!("read {}: {}", manifest_path.display(), err))?;
    serde_json::from_slice(&bytes).map_err(|err| format!("parse manifest: {err}"))
}

fn save_manifest(state_dir: &Path, manifest: &Manifest) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|err| err.to_string())?;
    let manifest_path = state_dir.join("daemon_state.json");
    std::fs::write(&manifest_path, bytes)
        .map_err(|err| format!("write {}: {}", manifest_path.display(), err))
}

fn reserve_vm_id(manifest: &mut Manifest) -> String {
    loop {
        let id = format!("vm-{}", manifest.next_id);
        manifest.next_id += 1;
        if !manifest.vms.contains_key(&id) {
            return id;
        }
    }
}

fn compare_vm_ids_newest_first(a: &str, b: &str) -> std::cmp::Ordering {
    vm_id_number(b).cmp(&vm_id_number(a)).then_with(|| b.cmp(a))
}

fn vm_id_number(id: &str) -> u64 {
    id.strip_prefix("vm-")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn ensure_capture_wrapper_program(state_dir: &Path) -> Result<PathBuf, String> {
    let program_path = state_dir.join(CAPTURE_WRAPPER_PROGRAM);
    let program = ProgramProbe {
        metadata: ProgramMetadataProbe {
            name: "AEON Capture Wrapper".to_string(),
        },
        instructions: capture_wrapper_instructions(),
    };
    let bytes = bincode::serialize(&program).map_err(|err| err.to_string())?;
    std::fs::write(&program_path, bytes)
        .map_err(|err| format!("write program {}: {}", program_path.display(), err))?;
    Ok(program_path)
}

fn capture_wrapper_program_id() -> Result<[u8; 32], String> {
    let instructions = capture_wrapper_instructions();
    let bytes = bincode::serialize(&instructions).map_err(|err| err.to_string())?;
    Ok(*blake3::hash(&bytes).as_bytes())
}

fn capture_wrapper_instructions() -> Vec<InstProbe> {
    vec![InstProbe::Halt]
}

fn snapshot_for_capture(
    entry: &CaptureEntry,
    program_id: [u8; 32],
) -> Result<SnapshotProbe, String> {
    let mut files = HashMap::new();
    files.insert("/capture/data".to_string(), entry.data.clone());
    files.insert(
        "/capture/meta.json".to_string(),
        capture_metadata_bytes(entry)?,
    );
    files.insert(
        "/capture/cid.hex".to_string(),
        hex_bytes(&entry.cid).into_bytes(),
    );
    files.insert("/capture/mime".to_string(), entry.mime().into_bytes());
    if let Some(title) = &entry.meta.title {
        files.insert("/capture/title.txt".to_string(), title.as_bytes().to_vec());
    }
    if let Ok(text) = std::str::from_utf8(&entry.data) {
        files.insert("/capture/text.txt".to_string(), text.as_bytes().to_vec());
    }

    let event = AeonEventProbe::Checkpoint {
        program_id,
        pc: 0,
        steps: 0,
    };
    let timestamp_ms = entry.captured_at;
    let prev_hash = [0u8; 32];
    let self_hash = hash_event_entry(&event, timestamp_ms, &prev_hash)?;

    Ok(SnapshotProbe {
        format_version: SNAPSHOT_FORMAT_VERSION,
        program_id,
        regs: vec![0; 256],
        pc: 0,
        call_stack: Vec::new(),
        steps: 0,
        heap: None,
        heap_top: Some(0),
        vfs: VirtualFsProbe {
            files,
            open_fds: Vec::new(),
        },
        event_log: EventLogProbe {
            entries: vec![LogEntryProbe {
                event,
                timestamp_ms,
                prev_hash,
                self_hash,
            }],
        },
    })
}

fn capture_metadata_bytes(entry: &CaptureEntry) -> Result<Vec<u8>, String> {
    let metadata = serde_json::json!({
        "cid": hex_bytes(&entry.cid),
        "kind": entry.kind.key(),
        "mime": entry.mime(),
        "title": entry.meta.title.clone(),
        "summary": entry.meta.summary.clone(),
        "app_name": entry.meta.app_name.clone(),
        "file_path": entry.meta.file_path.clone(),
        "url": entry.meta.url.clone(),
        "message_count": entry.meta.message_count,
        "previous_version": entry.meta.previous_version.map(|cid| hex_bytes(&cid)),
        "source": format!("{:?}", entry.source),
        "captured_at": entry.captured_at,
        "by": hex_bytes(&entry.by),
        "device": hex_bytes(&entry.device),
        "extra": entry.meta.extra.clone(),
    });
    serde_json::to_vec_pretty(&metadata).map_err(|err| err.to_string())
}

fn snapshot_to_bytes(snapshot: &SnapshotProbe) -> Result<Vec<u8>, String> {
    let payload = bincode::serialize(snapshot).map_err(|err| err.to_string())?;
    let mut out = blake3::hash(&payload).as_bytes().to_vec();
    out.extend_from_slice(&payload);
    Ok(out)
}

fn hash_event_entry(
    event: &AeonEventProbe,
    timestamp_ms: u64,
    prev_hash: &[u8; 32],
) -> Result<[u8; 32], String> {
    let payload =
        bincode::serialize(&(event, timestamp_ms, prev_hash)).map_err(|err| err.to_string())?;
    Ok(*blake3::hash(&payload).as_bytes())
}

fn hex_id(id: &[u8; 32]) -> String {
    hex_bytes(id)
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SnapshotProbe {
    format_version: u32,
    program_id: [u8; 32],
    regs: Vec<u64>,
    pc: usize,
    call_stack: Vec<usize>,
    steps: usize,
    heap: Option<Vec<u8>>,
    heap_top: Option<usize>,
    vfs: VirtualFsProbe,
    event_log: EventLogProbe,
}

impl SnapshotProbe {
    fn event_count(&self) -> usize {
        self.event_log.entries.len()
    }

    fn last_event(&self) -> Option<String> {
        self.event_log
            .entries
            .last()
            .map(|entry| format!("{:?}", entry.event))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct VirtualFsProbe {
    files: HashMap<String, Vec<u8>>,
    open_fds: Vec<Option<OpenFileProbe>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenFileProbe {
    path: String,
    cursor: usize,
    writable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EventLogProbe {
    entries: Vec<LogEntryProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LogEntryProbe {
    event: AeonEventProbe,
    timestamp_ms: u64,
    prev_hash: [u8; 32],
    self_hash: [u8; 32],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum AeonEventProbe {
    Checkpoint {
        program_id: [u8; 32],
        pc: usize,
        steps: usize,
    },
    VMMigrated {
        program_id: [u8; 32],
        from: String,
        to: String,
        steps: usize,
    },
    PatchApplied {
        context_id: String,
        author: String,
        description: String,
        patch_count: usize,
    },
    DaemonRestart {
        vm_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProgramProbe {
    metadata: ProgramMetadataProbe,
    instructions: Vec<InstProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProgramMetadataProbe {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum InstProbe {
    LoadImm { dst: u8, val: u64 },
    Mov { dst: u8, src: u8 },
    Add { dst: u8, a: u8, b: u8 },
    Sub { dst: u8, a: u8, b: u8 },
    Mul { dst: u8, a: u8, b: u8 },
    Jz { cond: u8, off: isize },
    Jump { offset: isize },
    Call { addr: usize },
    Ret,
    Print { r: u8 },
    LoadMem { dst: u8, addr: u8 },
    StoreMem { addr: u8, src: u8 },
    Alloc { dst: u8, size: u8 },
    Halt,
    Syscall { num: u8 },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-vm-capture-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn captures_daemon_vm_snapshot_bytes() {
        let dir = temp_dir();
        let snapshot = test_snapshot();
        let payload = bincode::serialize(&snapshot).unwrap();
        let mut bytes = blake3::hash(&payload).as_bytes().to_vec();
        bytes.extend_from_slice(&payload);
        let snapshot_path = dir.join("vm-1.snap");
        std::fs::write(&snapshot_path, &bytes).unwrap();
        write_manifest(&dir, &snapshot_path);

        let entry = capture_vm_snapshot_from_state_dir("vm-1", dir.clone()).unwrap();
        let vms = list_vms_in_state_dir(dir.clone()).unwrap();

        assert_eq!(entry.kind, CaptureKind::VmSnapshot);
        assert_eq!(entry.mime(), VM_SNAPSHOT_MIME);
        assert_eq!(entry.data, bytes);
        assert_eq!(
            entry.meta.extra.get("vm_id").map(String::as_str),
            Some("vm-1")
        );
        assert_eq!(entry.meta.extra.get("pc").map(String::as_str), Some("7"));
        assert_eq!(
            entry.meta.extra.get("steps").map(String::as_str),
            Some("42")
        );
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].snapshot_size, Some(entry.data.len()));
        assert_eq!(vms[0].steps, Some(42));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn wraps_capture_entry_as_managed_vm_snapshot() {
        let dir = temp_dir();
        let mut entry = CaptureEntry::new(
            b"hello from capture".to_vec(),
            CaptureKind::Text,
            CaptureSource::Manual,
        )
        .with_title("hello");

        let info = wrap_capture_entry_as_vm_snapshot_in_state_dir(&mut entry, dir.clone())
            .unwrap()
            .unwrap();
        let vms = list_vms_in_state_dir(dir.clone()).unwrap();
        let wrapped = capture_vm_snapshot_from_state_dir(&info.id, dir.clone()).unwrap();
        let probe = decode_snapshot_probe(&wrapped.data).unwrap();
        let first_event = &probe.event_log.entries[0];
        let expected_hash = hash_event_entry(
            &first_event.event,
            first_event.timestamp_ms,
            &first_event.prev_hash,
        )
        .unwrap();

        assert_eq!(
            entry.meta.extra.get("aeon_vm_wrapped").map(String::as_str),
            Some("true")
        );
        assert_eq!(entry.meta.extra.get("aeon_vm_id"), Some(&info.id));
        assert_eq!(vms.len(), 1);
        assert_eq!(vms[0].id, info.id);
        assert_eq!(wrapped.kind, CaptureKind::VmSnapshot);
        assert_eq!(
            probe.vfs.files.get("/capture/data").map(Vec::as_slice),
            Some(b"hello from capture".as_slice())
        );
        assert!(probe.vfs.files.contains_key("/capture/meta.json"));
        assert_eq!(first_event.prev_hash, [0u8; 32]);
        assert_eq!(first_event.self_hash, expected_hash);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn test_snapshot() -> SnapshotProbe {
        SnapshotProbe {
            format_version: 2,
            program_id: [7; 32],
            regs: vec![1, 2, 3],
            pc: 7,
            call_stack: vec![],
            steps: 42,
            heap: None,
            heap_top: Some(0),
            vfs: VirtualFsProbe::default(),
            event_log: EventLogProbe {
                entries: vec![LogEntryProbe {
                    event: AeonEventProbe::Checkpoint {
                        program_id: [7; 32],
                        pc: 7,
                        steps: 42,
                    },
                    timestamp_ms: 1,
                    prev_hash: [0; 32],
                    self_hash: [1; 32],
                }],
            },
        }
    }

    fn write_manifest(dir: &Path, snapshot_path: &Path) {
        let manifest = serde_json::json!({
            "next_id": 2,
            "vms": {
                "vm-1": {
                    "id": "vm-1",
                    "status": "paused",
                    "program_path": "demo.aeon",
                    "program_id": "070707",
                    "snapshot_path": snapshot_path.display().to_string(),
                }
            }
        });
        std::fs::write(
            dir.join("daemon_state.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }
}
