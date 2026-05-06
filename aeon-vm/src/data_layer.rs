use crate::editor::PatchSet;
use crate::{Program, Snapshot, VMState};
use aeon_store::{Blob, CIDStore, Node, CID};
use std::fmt;

pub const VM_SNAPSHOT_MIME: &str = "application/x-aeon-snapshot";
pub const VM_PROGRAM_MIME: &str = "application/x-aeon-program";
pub const VM_PATCHSET_MIME: &str = "application/x-aeon-patchset";
pub const AEON_NODE_MIME: &str = "application/x-aeon-node";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVMData {
    pub snapshot_cid: CID,
    pub program_cid: CID,
    pub snapshot_node_blob_cid: CID,
    pub program_node_blob_cid: CID,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPatchData {
    pub patchset_cid: CID,
    pub patch_node_blob_cid: CID,
}

#[derive(Debug)]
pub enum DataLayerError {
    Io(std::io::Error),
    Encode(String),
    MissingCid(CID),
}

pub fn store_vm_snapshot(
    store: &mut CIDStore,
    state: &VMState,
    program: &Program,
    session: &str,
) -> Result<StoredVMData, DataLayerError> {
    let snapshot = Snapshot::capture(state);
    let snapshot_cid = store.put(Blob::new(snapshot.to_bytes(), VM_SNAPSHOT_MIME))?;
    let program_cid = store.put(Blob::new(encode_program(program)?, VM_PROGRAM_MIME))?;

    let program_node =
        Node::new(program_cid, "vm-program", session).with_name(&program.metadata.name);
    let snapshot_node = Node::new(snapshot_cid, "vm-snapshot", session)
        .with_name(&format!("snapshot-steps-{}", snapshot.steps))
        .link_to(program_cid, "program");

    let program_node_blob_cid = store.put(node_blob(&program_node)?)?;
    let snapshot_node_blob_cid = store.put(node_blob(&snapshot_node)?)?;

    Ok(StoredVMData {
        snapshot_cid,
        program_cid,
        snapshot_node_blob_cid,
        program_node_blob_cid,
    })
}

pub fn store_patchset(
    store: &mut CIDStore,
    patchset: &PatchSet,
    base_snapshot_cid: CID,
    session: &str,
) -> Result<StoredPatchData, DataLayerError> {
    let patchset_cid = store.put(Blob::new(patchset.to_bytes(), VM_PATCHSET_MIME))?;
    let node = Node::new(patchset_cid, "vm-patchset", session)
        .with_name(&patchset.description)
        .link_to(base_snapshot_cid, "applies-to");
    let patch_node_blob_cid = store.put(node_blob(&node)?)?;

    Ok(StoredPatchData {
        patchset_cid,
        patch_node_blob_cid,
    })
}

pub fn load_snapshot(store: &mut CIDStore, cid: CID) -> Result<Snapshot, DataLayerError> {
    let blob = store.get(&cid)?.ok_or(DataLayerError::MissingCid(cid))?;
    Snapshot::from_bytes(&blob.data).map_err(|e| DataLayerError::Encode(e.to_string()))
}

pub fn load_program(store: &mut CIDStore, cid: CID) -> Result<Program, DataLayerError> {
    let blob = store.get(&cid)?.ok_or(DataLayerError::MissingCid(cid))?;
    Program::from_bytes(&blob.data).map_err(|e| DataLayerError::Encode(e.to_string()))
}

fn encode_program(program: &Program) -> Result<Vec<u8>, DataLayerError> {
    bincode::serialize(program).map_err(|e| DataLayerError::Encode(e.to_string()))
}

fn node_blob(node: &Node) -> Result<Blob, DataLayerError> {
    serde_json::to_vec(node)
        .map(|bytes| Blob::new(bytes, AEON_NODE_MIME))
        .map_err(|e| DataLayerError::Encode(e.to_string()))
}

impl fmt::Display for DataLayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataLayerError::Io(err) => write!(f, "{err}"),
            DataLayerError::Encode(err) => write!(f, "{err}"),
            DataLayerError::MissingCid(cid) => {
                write!(f, "missing CID {}", aeon_store::hex_cid(cid))
            }
        }
    }
}

impl std::error::Error for DataLayerError {}

impl From<std::io::Error> for DataLayerError {
    fn from(err: std::io::Error) -> Self {
        DataLayerError::Io(err)
    }
}
