use crate::{collab::CollabDoc, protocol::{SignedEnvelope, SyncMsg}, state::SyncState};
use aeon_store::{hex_cid, parse_cid_hex, Blob, CIDStore, DeviceInfo, Identity, Platform, SignedBlob, CID};
use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use ed25519_dalek::{Signature, VerifyingKey, Verifier};

#[derive(Clone)]
pub struct PeerConn {
    pub addr: String,
    writer: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
}

impl PeerConn {
    pub async fn send(&self, env: &SignedEnvelope) -> io::Result<()> {
        let mut w = self.writer.lock().await;
        let line = serde_json::to_string(env).map_err(io::Error::other)? + "\n";
        w.write_all(line.as_bytes()).await
    }
}

#[derive(Clone)]
pub struct SyncEngine {
    pub identity: Arc<Identity>,
    pub device: DeviceInfo,
    pub store: Arc<Mutex<CIDStore>>,
    pub peers: Arc<RwLock<HashMap<String, PeerConn>>>,
    pub collab_docs: Arc<Mutex<HashMap<CID, CollabDoc>>>,
    pub known_keys: Arc<Mutex<HashMap<[u8;32], VerifyingKey>>>,
    pub seen_nonces: Arc<Mutex<HashSet<([u8;32], u64)>>>,
    pub state: Arc<Mutex<SyncState>>,
    pub state_path: PathBuf,
    pub sync_root: PathBuf,
}

impl SyncEngine {
    pub fn new(identity: Arc<Identity>, device: DeviceInfo, store: CIDStore, state: SyncState, state_path: PathBuf, sync_root: PathBuf) -> Self {
        Self {
            identity,
            device,
            store: Arc::new(Mutex::new(store)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            collab_docs: Arc::new(Mutex::new(HashMap::new())),
            known_keys: Arc::new(Mutex::new(HashMap::new())),
            seen_nonces: Arc::new(Mutex::new(HashSet::new())),
            state: Arc::new(Mutex::new(state)),
            state_path,
            sync_root,
        }
    }

    pub async fn listen(self: Arc<Self>, bind: &str) -> io::Result<()> {
        let listener = TcpListener::bind(bind).await?;
        loop {
            let (stream, addr) = listener.accept().await?;
            let engine = self.clone();
            tokio::spawn(async move {
                let _ = engine.handle_incoming(stream, addr.to_string()).await;
            });
        }
    }

    pub async fn has_peer(&self, addr: &str) -> bool {
        self.peers.read().await.contains_key(addr)
    }

    pub async fn connect(self: Arc<Self>, addr: &str) -> io::Result<()> {
        let stream = TcpStream::connect(addr).await?;
        self.handle_incoming(stream, addr.to_string()).await
    }

    async fn handle_incoming(self: Arc<Self>, stream: TcpStream, addr: String) -> io::Result<()> {
        let (reader, writer) = stream.into_split();
        let peer = PeerConn { addr: addr.clone(), writer: Arc::new(tokio::sync::Mutex::new(writer)) };
        self.peers.write().await.insert(addr.clone(), peer.clone());

        let hello = self.sign_msg(SyncMsg::Hello {
            identity_id: self.identity.id,
            device_id: self.device.device_id,
            device_name: self.device.name.clone(),
            platform: Platform::current(),
            public_key: self.identity.public_key.as_bytes().to_vec(),
        });
        peer.send(&hello).await?;

        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() { continue; }
            if let Ok(env) = serde_json::from_str::<SignedEnvelope>(&line) {
                if self.verify_env(&env) {
                    self.on_message(env.msg, &peer).await;
                }
            }
        }
        self.peers.write().await.remove(&addr);
        Ok(())
    }


    fn sign_msg(&self, msg: SyncMsg) -> SignedEnvelope {
        let at = now_ms();
        let nonce = at ^ 0x5A17_u64;
        let by = self.identity.id;
        let payload = SignedEnvelope::payload_bytes(&msg, by, at, nonce);
        let signature = self.identity.sign(&payload).to_bytes().to_vec();
        SignedEnvelope { msg, by, at, nonce, public_key: self.identity.public_key.as_bytes().to_vec(), signature }
    }

    fn verify_env(&self, env: &SignedEnvelope) -> bool {
        let now = now_ms();
        if env.at + 300_000 < now || env.at > now + 300_000 { return false; }
        if env.public_key.len() != 32 || env.signature.len() != 64 { return false; }
        if *blake3::hash(&env.public_key).as_bytes() != env.by { return false; }
        let pk = match VerifyingKey::from_bytes(&env.public_key.clone().try_into().unwrap()) { Ok(v)=>v, Err(_)=>return false};
        let mut sig = [0u8;64]; sig.copy_from_slice(&env.signature);
        let sig = Signature::from_bytes(&sig);
        let payload = SignedEnvelope::payload_bytes(&env.msg, env.by, env.at, env.nonce);
        if pk.verify(&payload, &sig).is_err() { return false; }
        let mut st = self.state.lock().unwrap();
        st.cleanup_nonces(now, 600_000);
        if !st.trust_key(env.by, &env.public_key) { return false; }
        if st.nonce_seen(env.by, env.nonce) { return false; }
        st.mark_nonce(env.by, env.nonce, now);
        st.save(&self.state_path);
        true
    }

    pub async fn announce(&self, cid: CID) {
        let msg = SyncMsg::Have {
            identity_id: self.identity.id,
            device_id: self.device.device_id,
            cids: vec![cid],
            timestamp: now_ms(),
        };
        let peers = self.peers.read().await;
        for peer in peers.values() { let _ = peer.send(&self.sign_msg(msg.clone())).await; }
    }

    async fn on_message(&self, msg: SyncMsg, from: &PeerConn) {
        match msg {
            SyncMsg::Hello { identity_id, public_key, .. } => {
                if public_key.len() == 32 {
                    if let Ok(pk) = VerifyingKey::from_bytes(&public_key.clone().try_into().unwrap()) {
                        self.known_keys.lock().unwrap().insert(identity_id, pk);
                        let mut st = self.state.lock().unwrap();
                        if st.trust_key(identity_id, &public_key) {
                            st.save(&self.state_path);
                        }
                    }
                }
                let cids = self.store.lock().unwrap().list().unwrap_or_default();
                let _ = from.send(&self.sign_msg(SyncMsg::Have {
                    identity_id: self.identity.id,
                    device_id: self.device.device_id,
                    cids,
                    timestamp: now_ms(),
                })).await;
            }
            SyncMsg::Have { cids, .. } => {
                let missing: Vec<CID> = {
                    let store = self.store.lock().unwrap();
                    let st = self.state.lock().unwrap();
                    cids.into_iter().filter(|cid| !store.has(cid) && !st.has_seen(cid)).collect()
                };
                for cid in missing { let _ = from.send(&self.sign_msg(SyncMsg::Want { cid })).await; }
            }
            SyncMsg::Want { cid } => self.on_want(cid, from).await,
            SyncMsg::Data { blob } => self.on_data(blob).await,
            SyncMsg::FileIngest { path, cid, identity_id, device_id, mime, observed_at } => {
                let mut st = self.state.lock().unwrap();
                st.record_file_ingest(&path, cid, identity_id, device_id, &mime, observed_at);
                st.save(&self.state_path);
            }
            SyncMsg::Deleted { path, cid, at, .. } => {
                tracing::info!("peer deleted {} ({})", path, hex_cid(&cid));
                let mut st = self.state.lock().unwrap();
                st.add_tombstone(path.clone(), cid, at);
                st.save(&self.state_path);
                drop(st);
                self.apply_tombstone(path, at);
            }
            SyncMsg::Ping { .. } => {}
            SyncMsg::CollabPatch { doc_id, path, changes, by, at, nonce, public_key, signature } => {
                let _ = self.on_collab_patch(doc_id, path, changes, by, at, nonce, public_key, signature).await;
            }
        }
    }

    async fn on_want(&self, cid: CID, to: &PeerConn) {
        let got = self.store.lock().unwrap().get(&cid).ok().flatten();
        if let Some(blob) = got {
            let signed = SignedBlob {
                cid,
                data: blob.data.clone(),
                mime: blob.mime,
                created_by: self.identity.id,
                created_at: now_ms(),
                device_id: self.device.device_id,
                signature: self.identity.sign(&cid).to_bytes().to_vec(),
            };
            let _ = to.send(&self.sign_msg(SyncMsg::Data { blob: signed })).await;
        }
    }

    async fn on_data(&self, blob: SignedBlob) {
        if blob.data.len() > 10 * 1024 * 1024 { return; }
        let maybe_pk = self.known_keys.lock().unwrap().get(&blob.created_by).cloned();
        if let Some(pk) = maybe_pk {
            if blob.signature.len() == 64 {
                let mut sig = [0u8;64]; sig.copy_from_slice(&blob.signature);
                let sig = Signature::from_bytes(&sig);
                if pk.verify(&blob.cid, &sig).is_err() { return; }
            } else { return; }
        }
        let mut store = self.store.lock().unwrap();
        let _ = store.put(Blob { cid: blob.cid, data: blob.data, mime: blob.mime });
        drop(store);
        let mut st = self.state.lock().unwrap();
        st.mark_seen(&blob.cid);
        st.save(&self.state_path);
    }


    fn apply_tombstone(&self, path: String, at: u64) {
        let rel = PathBuf::from(&path);
        let target = if rel.is_absolute() { rel } else { self.sync_root.join(rel) };
        if !target.starts_with(&self.sync_root) { return; }
        let meta = std::fs::metadata(&target).ok();
        if let Some(meta) = meta {
            let local_ms = meta.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            // conflict rule: local newer content wins
            if local_ms > at {
                tracing::warn!("tombstone conflict kept local newer file: {}", target.display());
                return;
            }
        }
        let _ = std::fs::remove_file(&target);
    }

    pub fn replay_tombstones(&self) {
        let st = self.state.lock().unwrap().clone();
        for t in st.tombstones {
            self.apply_tombstone(t.path, t.at);
        }
    }

    pub async fn announce_file_ingest(&self, path: String, cid: CID, mime: String, observed_at: u64) {
        let msg = SyncMsg::FileIngest {
            path,
            cid,
            identity_id: self.identity.id,
            device_id: self.device.device_id,
            mime,
            observed_at,
        };
        let peers = self.peers.read().await;
        for peer in peers.values() { let _ = peer.send(&self.sign_msg(msg.clone())).await; }
    }

    pub async fn announce_delete(&self, path: String, cid: CID) {
        let at = now_ms();
        let msg = SyncMsg::Deleted { path: path.clone(), cid, by: self.identity.id, at, nonce: at ^ 0xD311_u64, public_key: self.identity.public_key.as_bytes().to_vec(), signature: self.identity.sign(path.as_bytes()).to_bytes().to_vec() };
        { let mut st = self.state.lock().unwrap(); st.add_tombstone(path, cid, at); st.save(&self.state_path); }
        let peers = self.peers.read().await;
        for peer in peers.values() { let _ = peer.send(&self.sign_msg(msg.clone())).await; }
    }
}

impl SyncEngine {
    pub async fn announce_collab_patch(&self, doc_id: CID, path: String, changes: Vec<u8>) {
        let at = now_ms();
        let nonce = at ^ 0xA30Au64;
        let mut sign_input = Vec::new();
        sign_input.extend_from_slice(&doc_id);
        sign_input.extend_from_slice(path.as_bytes());
        sign_input.extend_from_slice(&changes);
        sign_input.extend_from_slice(&nonce.to_le_bytes());
        sign_input.extend_from_slice(&at.to_le_bytes());
        let signature = self.identity.sign(&sign_input).to_bytes().to_vec();
        let msg = SyncMsg::CollabPatch {
            doc_id,
            path,
            changes,
            by: self.identity.id,
            at,
            nonce,
            public_key: self.identity.public_key.as_bytes().to_vec(),
            signature,
        };
        let peers = self.peers.read().await;
        for peer in peers.values() {
            let _ = peer.send(&self.sign_msg(msg.clone())).await;
        }
    }

    async fn on_collab_patch(
        &self,
        doc_id: CID,
        path: String,
        changes: Vec<u8>,
        by: [u8; 32],
        at: u64,
        nonce: u64,
        public_key: Vec<u8>,
        signature: Vec<u8>,
    ) -> io::Result<()> {
        let now = now_ms();
        if at + 300_000 < now || at > now + 300_000 { return Ok(()); }
        {
            let mut seen = self.seen_nonces.lock().unwrap();
            if seen.contains(&(by, nonce)) { return Ok(()); }
            seen.insert((by, nonce));
        }
        if changes.len() > 2 * 1024 * 1024 { return Ok(()); }
        if public_key.len() != 32 || signature.len() != 64 { return Ok(()); }
        let pk = VerifyingKey::from_bytes(&public_key.clone().try_into().unwrap()).map_err(io::Error::other)?;
        let mut sig_arr = [0u8;64]; sig_arr.copy_from_slice(&signature);
        let sig = Signature::from_bytes(&sig_arr);
        let mut sign_input = Vec::new();
        sign_input.extend_from_slice(&doc_id);
        sign_input.extend_from_slice(path.as_bytes());
        sign_input.extend_from_slice(&changes);
        sign_input.extend_from_slice(&nonce.to_le_bytes());
        sign_input.extend_from_slice(&at.to_le_bytes());
        if pk.verify(&sign_input, &sig).is_err() { return Ok(()); }
        if *blake3::hash(pk.as_bytes()).as_bytes() != by { return Ok(()); }

        let mut docs = self.collab_docs.lock().unwrap();
        let doc = docs.entry(doc_id).or_insert_with(|| CollabDoc::new(""));
        if doc.merge(&changes).is_ok() {
            let content = doc.content();
            let data = content.into_bytes();
            let blob = Blob { cid: *blake3::hash(&data).as_bytes(), data, mime: "text/plain".to_string() };
            self.store.lock().unwrap().put(blob)?;
            let mut st = self.state.lock().unwrap();
            let doc_hex = aeon_store::hex_cid(&doc_id);
            st.metric_mut(&doc_hex).applied_patches += 1;
            if doc.should_compact(64) {
                let snap = doc.snapshot_bytes();
                let snap_path = self.sync_root.join(".aeon-collab").join(format!("{}.snap", doc_hex));
                if let Some(p) = snap_path.parent() { let _ = std::fs::create_dir_all(p); }
                let _ = std::fs::write(snap_path, snap);
                st.metric_mut(&doc_hex).compacted_snapshots += 1;
            }
            st.save(&self.state_path);
            tracing::info!("collab merged for {}", path);
        }
        Ok(())
    }
}

fn now_ms() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }

pub fn device_id_from_name(name: &str) -> [u8; 16] {
    let hash = blake3::hash(name.as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&hash.as_bytes()[..16]);
    id
}

pub fn parse_cid_list(input: &[String]) -> Vec<CID> {
    input.iter().filter_map(|x| parse_cid_hex(x).ok()).collect()
}


impl SyncEngine {
    pub fn load_collab_snapshot(&self, doc_id: CID) {
        let hex = aeon_store::hex_cid(&doc_id);
        let path = self.sync_root.join(".aeon-collab").join(format!("{}.snap", hex));
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(doc) = CollabDoc::from_snapshot(&bytes) {
                self.collab_docs.lock().unwrap().insert(doc_id, doc);
            }
        }
    }
}
