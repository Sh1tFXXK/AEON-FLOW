use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::editor::PatchSet;
use crate::snapshot::Snapshot;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new(username: &str, device: &str, conversation: &str) -> Self {
        SessionId(format!("{}@{}/{}", username, device, conversation))
    }

    pub fn from_str(value: &str) -> Self {
        SessionId(value.to_string())
    }

    pub fn display(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Lamport(pub u64);

impl Lamport {
    pub fn zero() -> Self {
        Lamport(0)
    }

    pub fn next(&self) -> Self {
        Lamport(self.0 + 1)
    }

    pub fn advance(&self, other: Lamport) -> Self {
        Lamport(self.0.max(other.0) + 1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributedPatch {
    pub author: SessionId,
    pub clock: Lamport,
    pub wall_ms: u64,
    pub message: String,
    pub patchset: PatchSet,
}

impl AttributedPatch {
    pub fn new(
        author: SessionId,
        clock: Lamport,
        message: impl Into<String>,
        patchset: PatchSet,
    ) -> Self {
        AttributedPatch {
            author,
            clock,
            wall_ms: now_ms(),
            message: message.into(),
            patchset,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "[t={}] {} - \"{}\" ({} patch(es))",
            self.clock.0,
            self.author,
            self.message,
            self.patchset.len()
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub author: SessionId,
    pub clock: Lamport,
    pub wall_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedContext {
    pub id: String,
    pub base_snapshot: Snapshot,
    pub patches: Vec<AttributedPatch>,
    pub messages: Vec<SessionMessage>,
    pub connected_sessions: Vec<SessionId>,
}

impl SharedContext {
    pub fn new(id: impl Into<String>, base: Snapshot, creator: SessionId) -> Self {
        SharedContext {
            id: id.into(),
            base_snapshot: base,
            patches: Vec::new(),
            messages: Vec::new(),
            connected_sessions: vec![creator],
        }
    }

    pub fn current_snapshot(&self) -> Result<Snapshot, String> {
        let mut snap = self.base_snapshot.clone();
        for patch in &self.patches {
            snap = patch
                .patchset
                .apply(&snap)
                .map_err(|err| format!("patch by {} failed: {}", patch.author, err))?;
        }
        Ok(snap)
    }

    pub fn apply_patch(
        &mut self,
        author: SessionId,
        message: impl Into<String>,
        patchset: PatchSet,
    ) -> Result<Lamport, String> {
        let current = self.current_snapshot()?;
        patchset
            .apply(&current)
            .map_err(|err| format!("patch validation failed: {}", err))?;

        let clock = self.next_clock();
        self.patches
            .push(AttributedPatch::new(author, clock, message, patchset));
        Ok(clock)
    }

    pub fn post_message(&mut self, author: SessionId, text: impl Into<String>) -> Lamport {
        let clock = self.next_clock();
        self.messages.push(SessionMessage {
            author,
            clock,
            wall_ms: now_ms(),
            text: text.into(),
        });
        clock
    }

    pub fn join(&mut self, session: SessionId) {
        if !self.connected_sessions.contains(&session) {
            self.connected_sessions.push(session);
        }
    }

    pub fn leave(&mut self, session: &SessionId) {
        self.connected_sessions.retain(|item| item != session);
    }

    pub fn print_timeline(&self) {
        let mut events: Vec<(Lamport, String)> = Vec::new();

        for patch in &self.patches {
            events.push((patch.clock, format!("PATCH {}", patch.summary())));
        }
        for message in &self.messages {
            events.push((
                message.clock,
                format!(
                    "MSG [t={}] {} - \"{}\"",
                    message.clock.0, message.author, message.text
                ),
            ));
        }

        events.sort_by_key(|(clock, _)| *clock);
        for (_, event) in events {
            println!("{}", event);
        }
    }

    pub fn patch_count(&self) -> usize {
        self.patches.len()
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        bincode::deserialize(data).map_err(|err| err.to_string())
    }

    fn next_clock(&self) -> Lamport {
        let max_patch = self
            .patches
            .iter()
            .map(|patch| patch.clock)
            .max()
            .unwrap_or_else(Lamport::zero);
        let max_message = self
            .messages
            .iter()
            .map(|message| message.clock)
            .max()
            .unwrap_or_else(Lamport::zero);
        Lamport(max_patch.0.max(max_message.0) + 1)
    }
}

pub struct ContextRegistry {
    inner: RwLock<HashMap<String, Arc<RwLock<SharedContext>>>>,
}

impl ContextRegistry {
    pub fn new() -> Self {
        ContextRegistry {
            inner: RwLock::new(HashMap::new()),
        }
    }

    pub fn create(
        &self,
        id: impl Into<String>,
        base: Snapshot,
        creator: SessionId,
    ) -> Arc<RwLock<SharedContext>> {
        let id = id.into();
        let context = Arc::new(RwLock::new(SharedContext::new(id.clone(), base, creator)));
        self.inner.write().unwrap().insert(id, context.clone());
        context
    }

    pub fn get(&self, id: &str) -> Option<Arc<RwLock<SharedContext>>> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<String> {
        self.inner.read().unwrap().keys().cloned().collect()
    }
}

impl Default for ContextRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
