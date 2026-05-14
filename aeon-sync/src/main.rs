use aeon_capture::{apps, CaptureEngine, CaptureStore};
use aeon_store::{CIDStore, Identity};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

mod process;
mod server;
mod watcher;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let home = dirs::home_dir().expect("Cannot find home directory");
    let sync_dir = home.join("AEON");
    std::fs::create_dir_all(&sync_dir).expect("failed to create sync directory");

    let aeon_dir = home.join(".aeon");
    let store_dir = aeon_dir.join("store");
    std::fs::create_dir_all(&store_dir).expect("failed to create store directory");

    let identity_path = aeon_dir.join("identity");
    let identity = Identity::load_or_create(&identity_path).expect("failed to load identity");
    let device_id = local_device_id();

    let capture_store = CaptureStore::new(
        CIDStore::new(store_dir).expect("failed to open CID store"),
        aeon_dir.join("capture-index.json"),
    )
    .expect("failed to open capture store");
    let capture_engine = Arc::new(CaptureEngine::new_with_identity(
        Arc::new(Mutex::new(capture_store)),
        identity.id,
        device_id,
    ));

    #[cfg(target_os = "windows")]
    {
        let engine = capture_engine.clone();
        tokio::spawn(async move {
            aeon_capture::clipboard::start_clipboard_monitor(engine).await;
        });
    }

    let _screenshot_watcher =
        aeon_capture::screenshot::start_screenshot_monitor(capture_engine.clone()).ok();
    let _capture_file_watcher =
        aeon_capture::file::start_file_monitor(capture_engine.clone(), vec![sync_dir.clone()]).ok();
    let app_registry = Arc::new(apps::default_registry(capture_engine.clone()));

    let (file_events, _) = broadcast::channel(128);
    let _watcher =
        watcher::start_watcher(&sync_dir, file_events.clone()).expect("watcher start failed");

    let state = server::AppState {
        sync_dir: sync_dir.clone(),
        file_events,
        identity_short: identity.id_short(),
        identity_id: identity.id,
        device_id,
        capture_engine,
        app_registry,
        devices: Arc::new(Mutex::new(server::DeviceRegistry::default())),
    };

    let app = server::create_router(state);
    let local_ip = local_ip_address().unwrap_or_else(|| "127.0.0.1".to_string());
    let port = 8080u16;
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().expect("invalid addr");

    println!("\nAEON Flow capture service started");
    println!("----------------------------------------");
    println!("Identity ID: {}", identity.id_short());
    println!("Sync dir:    {}", sync_dir.display());
    println!("LAN:         http://{}:{}", local_ip, port);
    println!("Local:       http://localhost:{}", port);
    println!("----------------------------------------");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    axum::serve(listener, app).await.expect("server failed");
}

fn local_ip_address() -> Option<String> {
    #[cfg(target_os = "windows")]
    if let Some(ip) = windows_lan_ip_address() {
        return Some(ip);
    }
    #[cfg(target_os = "windows")]
    if let Some(ip) = ipconfig_lan_ip_address() {
        return Some(ip);
    }

    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

#[cfg(target_os = "windows")]
fn windows_lan_ip_address() -> Option<String> {
    let script = r#"
Get-NetIPAddress -AddressFamily IPv4 |
  Where-Object {
    $_.AddressState -eq 'Preferred' -and
    $_.IPAddress -notlike '127.*' -and
    $_.InterfaceAlias -notmatch 'VMware|Virtual|vEthernet|Tailscale|Meta|Loopback|Docker|WSL'
  } |
  Sort-Object @{Expression={ if ($_.InterfaceAlias -match 'WLAN|Wi-Fi|Ethernet') { 0 } else { 1 } }}, InterfaceAlias |
  Select-Object -First 1 -ExpandProperty IPAddress
"#;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(target_os = "windows")]
fn ipconfig_lan_ip_address() -> Option<String> {
    let output = std::process::Command::new("ipconfig").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut candidates: Vec<std::net::Ipv4Addr> = text.lines().filter_map(extract_ipv4).collect();
    candidates.sort_by_key(ipv4_score);
    candidates.first().map(ToString::to_string)
}

#[cfg(target_os = "windows")]
fn extract_ipv4(line: &str) -> Option<std::net::Ipv4Addr> {
    line.split(|c: char| !(c.is_ascii_digit() || c == '.'))
        .filter(|part| part.matches('.').count() == 3)
        .filter_map(|part| part.parse().ok())
        .find(is_lan_candidate)
}

#[cfg(target_os = "windows")]
fn is_lan_candidate(ip: &std::net::Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    if a == 127 || (a == 169 && b == 254) || (a == 198 && b == 18) {
        return false;
    }
    if a == 100 && (64..=127).contains(&b) {
        return false;
    }
    a == 10 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

#[cfg(target_os = "windows")]
fn ipv4_score(ip: &std::net::Ipv4Addr) -> u8 {
    let [a, b, _, d] = ip.octets();
    let subnet_score = if a == 192 && b == 168 {
        0
    } else if a == 10 {
        10
    } else {
        20
    };
    subnet_score + if d == 1 { 50 } else { 0 }
}

fn local_device_id() -> [u8; 16] {
    let source = std::env::var("AEON_DEVICE_ID")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "aeon-local-device".to_string());
    let hash = blake3::hash(source.as_bytes());
    let mut id = [0u8; 16];
    id.copy_from_slice(&hash.as_bytes()[..16]);
    id
}
