pub type CID = [u8; 32];

pub mod account;
pub mod blob;
pub mod chat;
pub mod context;
pub mod event;
pub mod kind;
pub mod link;
pub mod node;
pub mod store;
pub mod sync;

pub use account::{Account, AccountId};
pub use blob::{mime_from_path, Blob};
pub use chat::{Message, Thread, MESSAGE_MIME, THREAD_MIME};
pub use context::{Context, ContextError, ContextEvent, CONTEXT_MIME};
pub use event::DataEvent;
pub use kind::{DataDescriptor, DataKind};
pub use link::Link;
pub use node::Node;
pub use store::{hex_cid, parse_cid_hex, CIDStore};
pub use sync::{pack_cids, unpack_cids, SyncEngine, SyncMessage, SyncReport};

#[cfg(test)]
mod tests;
