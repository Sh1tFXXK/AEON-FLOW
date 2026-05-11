mod discovery;
mod engine;
mod protocol;
mod watcher;

use aeon_store::{DeviceInfo, Identity, Platform, CIDStore};
use engine::{device_id_from_name, SyncEngine};
use std::sync::Arc;
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

    let engine = Arc::new(SyncEngine::new(Arc::new(identity), device, store));
    let listen_addr = std::env::var("AEON_AGENT_LISTEN").unwrap_or_else(|_| "0.0.0.0:8787".to_string());
    let accept_engine = engine.clone();
    tokio::spawn(async move { let _ = accept_engine.listen(&listen_addr).await; });

    if let Ok(peer) = std::env::var("AEON_AGENT_PEER") {
        let connect_engine = engine.clone();
        tokio::spawn(async move { let _ = connect_engine.connect(&peer).await; });
    }

    tracing::info!("agent started: {}", engine.identity.id_short());
    while let Some(ev) = rx.recv().await {
        tracing::info!("event: {:?}", ev);
    }
}
