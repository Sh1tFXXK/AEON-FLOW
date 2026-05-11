use aeon_store::Identity;
use std::net::SocketAddr;
use tokio::sync::broadcast;

mod server;
mod watcher;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).compact().init();

    let home = dirs::home_dir().expect("Cannot find home directory");
    let sync_dir = home.join("AEON");
    std::fs::create_dir_all(&sync_dir).expect("failed to create sync directory");

    let store_dir = home.join(".aeon").join("store");
    std::fs::create_dir_all(&store_dir).expect("failed to create store directory");

    let identity_path = home.join(".aeon").join("identity");
    let identity = Identity::load_or_create(&identity_path).expect("failed to load identity");

    let (file_events, _) = broadcast::channel(128);
    let _watcher = watcher::start_watcher(&sync_dir, file_events.clone()).expect("watcher start failed");

    let state = server::AppState {
        sync_dir: sync_dir.clone(),
        file_events,
        identity_short: identity.id_short(),
    };

    let app = server::create_router(state);
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = 8080u16;
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().expect("invalid addr");

    println!("\n⬡ AEON Flow 文件同步服务已启动");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("身份 ID:  {}", identity.id_short());
    println!("同步目录:  {}", sync_dir.display());
    println!("局域网:    http://{}:{}", local_ip, port);
    println!("本机访问:  http://localhost:{}", port);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let listener = tokio::net::TcpListener::bind(addr).await.expect("bind failed");
    axum::serve(listener, app).await.expect("server failed");
}

fn local_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}
