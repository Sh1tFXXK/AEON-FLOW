use crate::CID;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DataEvent {
    BlobAdded {
        cid: CID,
        mime: String,
        size_bytes: usize,
        by: String,
    },
    NodeCreated {
        node_cid: CID,
        kind: String,
        name: Option<String>,
        by: String,
    },
    NodeUpdated {
        old_cid: CID,
        new_cid: CID,
        diff_description: String,
        by: String,
    },
    LinkAdded {
        from: CID,
        to: CID,
        label: String,
        by: String,
    },
    SharedToContext {
        node_cid: CID,
        context_id: String,
        by: String,
    },
    Merged {
        sources: Vec<CID>,
        result: CID,
        by: String,
    },
}
