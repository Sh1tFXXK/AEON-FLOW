mod collab;
mod discovery;
mod engine;
mod protocol;
mod state;
mod watcher;
mod tunnel;

use aeon_store::{Blob, DeviceInfo, Identity, Platform, CIDStore};
use engine::{device_id_from_name, SyncEngine};
use state::SyncState;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use tokio::sync::mpsc;
use tunnel::TunnelConfig;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).compact().init();

    let home = dirs::home_dir().expect("missing home dir");
    let identity = Identity::load_or_create(&home.join(".aeon").join("identity")).expect("identity");

    let discovery = discovery::Discovery { identity_id: identity.id, port: 8787 };
    let mdns = discovery.announce().expect("mdns announce failed");

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

    if let Ok(rx) = discovery::Discovery::browse(&mdns) {
        let (peer_tx, mut peer_rx) = mpsc::unbounded_channel::<String>();
        let local_id = engine.identity.id;
        thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                if let mdns_sd::ServiceEvent::ServiceResolved(info) = event {
                    let fullname = info.get_fullname().to_string();
                    let self_tag = hex::encode(&local_id[..4]);
                    if fullname.contains(&self_tag) {
                        continue;
                    }
                    for addr in info.get_addresses() {
                        let _ = peer_tx.send(format!("{}:{}", addr, info.get_port()));
                    }
                }
            }
        });

        let discover_engine = engine.clone();
        tokio::spawn(async move {
            let mut recently_tried: HashSet<String> = HashSet::new();
            while let Some(addr) = peer_rx.recv().await {
                if recently_tried.contains(&addr) {
                    continue;
                }
                recently_tried.insert(addr.clone());
                if !discover_engine.has_peer(&addr).await {
                    let _ = discover_engine.clone().connect(&addr).await;
                }
                if recently_tried.len() > 1024 {
                    recently_tried.clear();
                }
            }
        });
    }
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

    // optional WAN/bootstrap peers (e.g. Cloudflare Tunnel public addresses)
    let relay_peers = std::env::var("AEON_AGENT_RELAY_PEERS")
        .or_else(|_| std::env::var("AEON_AGENT_RELAY_PEER"))
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !relay_peers.is_empty() {
        let relay_engine = engine.clone();
        tokio::spawn(async move {
            loop {
                for peer in &relay_peers {
                    if !relay_engine.has_peer(peer).await {
                        let _ = relay_engine.clone().connect(peer).await;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
        });
    }


    if let Some(tunnel_cfg) = TunnelConfig::from_env() {
        let tunnel_engine = engine.clone();
        tokio::spawn(async move {
            loop {
                let healthy = tunnel::health_check(&tunnel_cfg.endpoint).await;
                {
                    let mut st = tunnel_engine.state.lock().unwrap();
                    st.set_tunnel_status(tunnel_cfg.provider.clone(), tunnel_cfg.endpoint.clone(), healthy, if healthy {"healthy".to_string()} else {"unreachable".to_string()});
                    st.save(&tunnel_engine.state_path);
                }
                if healthy && !tunnel_engine.has_peer(&tunnel_cfg.endpoint).await {
                    let _ = tunnel_engine.clone().connect(&tunnel_cfg.endpoint).await;
                }
                if !healthy {
                    // fallback: keep trying relay peers/manual peers loops already running.
                }
                tokio::time::sleep(std::time::Duration::from_secs(tunnel_cfg.health_interval_secs)).await;
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
                            let path_str = path.to_string_lossy().to_string();
                            file_index.lock().unwrap().insert(path_str.clone(), signed.cid);
                            {
                                let mut st = engine.state.lock().unwrap();
                                st.record_file_ingest(&path_str, signed.cid, engine.identity.id, engine.device.device_id, &blob.mime, signed.created_at);
                                st.save(&engine.state_path);
                            }
                            engine.announce(signed.cid).await;
                            engine.announce_file_ingest(path_str.clone(), signed.cid, blob.mime.clone(), signed.created_at).await;
                            if blob.mime.starts_with("text/") || blob.mime == "application/json" {
                                let p = path_str.clone();
                                let (doc_id, changes) = {
                                    let mut st = engine.state.lock().unwrap();
                                    let doc_id = st.collab_doc_for_path(&p);
                                    st.save(&engine.state_path);
                                    let mut doc = collab::CollabDoc::new(&String::from_utf8_lossy(&blob.data));
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
                let path_str = path.to_string_lossy().to_string();
                if let Some(cid) = file_index.lock().unwrap().remove(&path_str) {
                    {
                        let mut st = engine.state.lock().unwrap();
                        st.remove_file_record(&path_str);
                        st.save(&engine.state_path);
                    }
                    engine.announce_delete(path_str, cid).await;
                }
            }
        }
    }
}
