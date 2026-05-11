use std::net::SocketAddr;

mod server;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_target(false).compact().init();

    let sync_dir = dirs::home_dir()
        .expect("Cannot find home directory")
        .join("AEON");
    std::fs::create_dir_all(&sync_dir).expect("failed to create sync directory");
    tracing::info!("Sync directory: {}", sync_dir.display());

    let store_dir = dirs::home_dir()
        .expect("Cannot find home directory")
        .join(".aeon")
        .join("store");
    std::fs::create_dir_all(&store_dir).expect("failed to create store directory");
    tracing::info!("Store directory: {}", store_dir.display());

    let state = server::AppState {
        sync_dir: sync_dir.clone(),
    };

    let app = server::create_router(state);
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = 8080u16;
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().expect("invalid addr");

    println!("\n⬡ AEON Flow 文件同步服务已启动");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("同步目录:  {}", sync_dir.display());
    println!("局域网:    http://{}:{}", local_ip, port);
    println!("本机访问:  http://localhost:{}", port);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("把文件放入同步目录，手机浏览器打开上面的地址即可访问");
    println!("提示: 运行 cloudflared tunnel --url http://localhost:{} 可从外网访问\n", port);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("server failed");
}

fn local_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}
