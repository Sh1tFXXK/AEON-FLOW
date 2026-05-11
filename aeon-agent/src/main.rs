mod discovery;
mod protocol;
mod watcher;

use aeon_store::Identity;
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
    let _watcher = watcher::start(dirs, tx).expect("watcher start failed");

    tracing::info!("agent started: {}", identity.id_short());
    while let Some(ev) = rx.recv().await {
        tracing::info!("event: {:?}", ev);
    }
}
