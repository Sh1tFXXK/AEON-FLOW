pub type CID = [u8; 32];

pub mod blob;
pub mod event;
pub mod link;
pub mod node;
pub mod store;
pub mod sync;

pub use blob::{mime_from_path, Blob};
pub use event::DataEvent;
pub use link::Link;
pub use node::Node;
pub use store::{hex_cid, parse_cid_hex, CIDStore};
pub use sync::{pack_cids, unpack_cids, SyncEngine, SyncMessage, SyncReport};

#[cfg(test)]
mod tests;
