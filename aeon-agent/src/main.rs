mod collab;
mod discovery;
mod engine;
mod protocol;
mod state;
mod watcher;

use aeon_store::{Blob, DeviceInfo, Identity, Platform, CIDStore};
use engine::{device_id_from_name, SyncEngine};
use state::SyncState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).compact().init();

    let home = dirs::home_dir().expect("missing home dir");
    let identity = Identity::load_or_create(&home.join(".aeon").join("identity")).expect("identity");

    let discovery = discovery::Discovery { identity_id: identity.id, port: 8787 };
    let _mdns = discovery.announce().expect("mdns announce failed");

    let (tx, mut rx) = mpsc::channel(1024);
    let dirs = watcher::default_sync_dirs();
    let _watcher = watcher::start(dirs.clone(), tx).expect("watcher start failed");

    let store = CIDStore::new(CIDStore::default_path()).expect("store");
    let device_name = std::env::var("AEON_DEVICE_NAME").unwrap_or_else(|_| "local-device".to_string());
    let device = DeviceInfo {
        device_id: device_id_from_name(&device_name),
        identity_id: identity.id,
        name: device_name,
        platform: Platform::current(),
        last_seen: 0,
    };

    let state_path = home.join(".aeon").join("agent_state.json");
    let sync_state = SyncState::load(&state_path);
    let sync_root = dirs.get(0).cloned().unwrap_or_else(|| home.join("AEON"));
    let engine = Arc::new(SyncEngine::new(Arc::new(identity), device, store, sync_state, state_path, sync_root));
    engine.replay_tombstones();
    let listen_addr = std::env::var("AEON_AGENT_LISTEN").unwrap_or_else(|_| "0.0.0.0:8787".to_string());
    let accept_engine = engine.clone();
    tokio::spawn(async move { let _ = accept_engine.listen(&listen_addr).await; });

    let peers = std::env::var("AEON_AGENT_PEERS")
        .or_else(|_| std::env::var("AEON_AGENT_PEER"))
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();

    if !peers.is_empty() {
        let retry_engine = engine.clone();
        tokio::spawn(async move {
            loop {
                for peer in &peers {
                    if !retry_engine.has_peer(peer).await {
                        let _ = retry_engine.clone().connect(peer).await;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    }

    tracing::info!("agent started: {}", engine.identity.id_short());
    let file_index: Arc<Mutex<HashMap<String, [u8; 32]>>> = Arc::new(Mutex::new(engine.state.lock().unwrap().tombstone_map()));

    while let Some(ev) = rx.recv().await {
        match ev {
            watcher::FileEvent::Created { path } | watcher::FileEvent::Modified { path } => {
                if path.is_file() {
                    if let Ok(blob) = Blob::from_file(&path) {
                        let signed = engine.store.lock().unwrap().put_signed(
                            blob.data.clone(),
                            &blob.mime,
                            &engine.identity,
                            &engine.device,
                        );
                        if let Ok(signed) = signed {
                            file_index.lock().unwrap().insert(path.to_string_lossy().to_string(), signed.cid);
                            engine.announce(signed.cid).await;
                            if blob.mime.starts_with("text/") || blob.mime == "application/json" {
                                let p = path.to_string_lossy().to_string();
                                let (doc_id, changes) = {
                                    let mut st = engine.state.lock().unwrap();
                                    let doc_id = st.collab_doc_for_path(&p);
                                    st.save(&engine.state_path);
                                    let doc = collab::CollabDoc::new(&String::from_utf8_lossy(&blob.data));
                                    (doc_id, doc.save())
                                };
                                engine.announce_collab_patch(doc_id, p, changes).await;
                            }
                            tracing::info!("synced: {}", path.display());
                        }
                    }
                }
            }
            watcher::FileEvent::Deleted { path } => {
                if let Some(cid) = file_index.lock().unwrap().remove(&path.to_string_lossy().to_string()) {
                    engine.announce_delete(path.to_string_lossy().to_string(), cid).await;
                }
            }
        }
    }
}
