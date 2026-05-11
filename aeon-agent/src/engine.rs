use crate::{collab::CollabDoc, protocol::SyncMsg};
use aeon_store::{hex_cid, parse_cid_hex, Blob, CIDStore, DeviceInfo, Identity, Platform, SignedBlob, CID};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct PeerConn {
    pub addr: String,
    writer: Arc<tokio::sync::Mutex<tokio::net::tcp::OwnedWriteHalf>>,
}

impl PeerConn {
    pub async fn send(&self, msg: &SyncMsg) -> io::Result<()> {
        let mut w = self.writer.lock().await;
        let line = serde_json::to_string(msg).map_err(io::Error::other)? + "\n";
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
}

impl SyncEngine {
    pub fn new(identity: Arc<Identity>, device: DeviceInfo, store: CIDStore) -> Self {
        Self {
            identity,
            device,
            store: Arc::new(Mutex::new(store)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            collab_docs: Arc::new(Mutex::new(HashMap::new())),
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

        peer.send(&SyncMsg::Hello {
            identity_id: self.identity.id,
            device_id: self.device.device_id,
            device_name: self.device.name.clone(),
            platform: Platform::current(),
        }).await?;

        let mut lines = BufReader::new(reader).lines();
        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() { continue; }
            if let Ok(msg) = serde_json::from_str::<SyncMsg>(&line) {
                self.on_message(msg, &peer).await;
            }
        }
        self.peers.write().await.remove(&addr);
        Ok(())
    }

    pub async fn announce(&self, cid: CID) {
        let msg = SyncMsg::Have {
            identity_id: self.identity.id,
            device_id: self.device.device_id,
            cids: vec![cid],
            timestamp: now_ms(),
        };
        let peers = self.peers.read().await;
        for peer in peers.values() { let _ = peer.send(&msg).await; }
    }

    async fn on_message(&self, msg: SyncMsg, from: &PeerConn) {
        match msg {
            SyncMsg::Hello { .. } => {
                let cids = self.store.lock().unwrap().list().unwrap_or_default();
                let _ = from.send(&SyncMsg::Have {
                    identity_id: self.identity.id,
                    device_id: self.device.device_id,
                    cids,
                    timestamp: now_ms(),
                }).await;
            }
            SyncMsg::Have { cids, .. } => {
                let store = self.store.lock().unwrap();
                let missing: Vec<CID> = cids.into_iter().filter(|cid| !store.has(cid)).collect();
                drop(store);
                for cid in missing { let _ = from.send(&SyncMsg::Want { cid }).await; }
            }
            SyncMsg::Want { cid } => self.on_want(cid, from).await,
            SyncMsg::Data { blob } => self.on_data(blob).await,
            SyncMsg::Deleted { path, cid, .. } => {
                tracing::info!("peer deleted {} ({})", path, hex_cid(&cid));
            }
            SyncMsg::Ping { .. } => {}
            SyncMsg::CollabPatch { doc_id, path, changes, by, at } => {
                let _ = self.on_collab_patch(doc_id, path, changes, by, at).await;
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
            let _ = to.send(&SyncMsg::Data { blob: signed }).await;
        }
    }

    async fn on_data(&self, blob: SignedBlob) {
        let mut store = self.store.lock().unwrap();
        let _ = store.put(Blob { cid: blob.cid, data: blob.data, mime: blob.mime });
    }

    pub async fn announce_delete(&self, path: String, cid: CID) {
        let msg = SyncMsg::Deleted { path, cid, by: self.identity.id, at: now_ms() };
        let peers = self.peers.read().await;
        for peer in peers.values() { let _ = peer.send(&msg).await; }
    }
}


    pub async fn announce_collab_patch(&self, doc_id: CID, path: String, changes: Vec<u8>) {
        let msg = SyncMsg::CollabPatch {
            doc_id,
            path,
            changes,
            by: self.identity.id,
            at: now_ms(),
        };
        let peers = self.peers.read().await;
        for peer in peers.values() {
            let _ = peer.send(&msg).await;
        }
    }

    async fn on_collab_patch(
        &self,
        doc_id: CID,
        path: String,
        changes: Vec<u8>,
        _by: [u8; 32],
        _at: u64,
    ) -> io::Result<()> {
        let mut docs = self.collab_docs.lock().unwrap();
        let doc = docs.entry(doc_id).or_insert_with(|| CollabDoc::new(""));
        if doc.merge(&changes).is_ok() {
            let content = doc.content();
            let data = content.into_bytes();
            let blob = Blob { cid: *blake3::hash(&data).as_bytes(), data, mime: "text/plain".to_string() };
            self.store.lock().unwrap().put(blob)?;
            tracing::info!("collab merged for {}", path);
        }
        Ok(())
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
