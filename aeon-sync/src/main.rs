use aeon_capture::{apps, CaptureEngine, CaptureStore, EventLog};
use aeon_store::{CIDStore, Identity};
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex};

mod account_profiles;
mod bridge;
mod email_imap;
mod email_sync;
mod operation_context;
mod process;
mod query;
mod relay;
mod server;
mod vault;
mod watcher;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "relay") {
        args.remove(0);
        run_relay_mode(args).await;
        return;
    }
    if args.first().is_some_and(|arg| arg == "start") {
        args.remove(0);
    }
    let start_config = parse_start_args(args);

    let home = dirs::home_dir().expect("Cannot find home directory");
    let sync_dir = home.join("AEON");
    std::fs::create_dir_all(&sync_dir).expect("failed to create sync directory");

    let aeon_dir = home.join(".aeon");
    let store_dir = aeon_dir.join("store");
    std::fs::create_dir_all(&store_dir).expect("failed to create store directory");
    let event_log = Arc::new(Mutex::new(EventLog::new(aeon_dir.join("events.jsonl"))));
    let operation_context = Arc::new(Mutex::new(
        operation_context::ContextStore::new(aeon_dir.join("context.json"))
            .expect("failed to open operation context store"),
    ));
    let account_profiles = Arc::new(Mutex::new(
        account_profiles::AccountProfileStore::new(aeon_dir.join("account-profiles.json"))
            .expect("failed to open account profile store"),
    ));
    let credential_vault = Arc::new(Mutex::new(
        vault::CredentialVaultStore::new(aeon_dir.join("vault.json"))
            .expect("failed to open credential vault"),
    ));
    let vault_sessions = Arc::new(Mutex::new(vault::CredentialUnlockSessions::default()));
    let email_sync = Arc::new(Mutex::new(
        email_sync::EmailSyncStore::new(aeon_dir.join("email-sync.json"))
            .expect("failed to open email sync store"),
    ));
    let query_planner = match query::QueryPlannerConfig::from_env() {
        Ok(config) => config,
        Err(err) => {
            eprintln!("AEON query planner config ignored: {err:?}");
            None
        }
    };

    let identity_path = aeon_dir.join("identity");
    let identity = Identity::load_or_create(&identity_path).expect("failed to load identity");
    let device_id = local_device_id();

    let capture_store = CaptureStore::new(
        CIDStore::new(store_dir).expect("failed to open CID store"),
        aeon_dir.join("capture-index.json"),
    )
    .expect("failed to open capture store");
    let capture_engine = Arc::new(CaptureEngine::new_with_identity_and_events(
        Arc::new(Mutex::new(capture_store)),
        identity.id,
        device_id,
        Some(event_log.clone()),
    ));

    #[cfg(target_os = "windows")]
    {
        let engine = capture_engine.clone();
        tokio::spawn(async move {
            aeon_capture::clipboard::start_clipboard_monitor(engine).await;
        });
        let engine = capture_engine.clone();
        tokio::spawn(async move {
            aeon_capture::os_activity::start_foreground_window_monitor(engine).await;
        });
        let engine = capture_engine.clone();
        tokio::spawn(async move {
            aeon_capture::os_activity::start_text_commit_monitor(engine).await;
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

    if start_config.with_relay {
        spawn_embedded_relay(start_config.relay.clone()).await;
    }

    let port = start_config.port;
    let embedded_relay_url = start_config
        .with_relay
        .then(|| format!("http://127.0.0.1:{}", start_config.relay.port));
    let relay_url = start_config.relay_url.clone().or(embedded_relay_url);
    let relay_space = start_config.relay.space.clone();
    let device_name = local_device_name();
    let connect_urls = connection_urls(
        port,
        start_config.with_relay.then_some(start_config.relay.port),
        relay_url.as_deref(),
    );
    spawn_lan_discovery(LanDiscoveryConfig {
        port: start_config.discovery_port,
        ui_port: port,
        relay_port: start_config.with_relay.then_some(start_config.relay.port),
        device_id: hex_bytes(&device_id),
        device_name: device_name.clone(),
        identity_short: identity.id_short(),
    });
    if let Some(url) = relay_url.clone() {
        relay::spawn_pull_loop(relay::RelayPullConfig {
            url: url.clone(),
            space: relay_space.clone(),
            device_id: hex_bytes(&device_id),
            device_name: device_name.clone(),
            cursor_path: aeon_dir.join("relay-cursor.txt"),
            capture_engine: capture_engine.clone(),
        });
        println!("AEON Relay pull enabled: {url} (space: {relay_space})");
        relay::spawn_push_loop(relay::RelayPushConfig {
            url,
            space: relay_space.clone(),
            device_id: hex_bytes(&device_id),
            device_name: device_name.clone(),
            device_kind: "desktop".to_string(),
            capture_engine: capture_engine.clone(),
        });
        println!("AEON Relay push enabled (space: {relay_space})");
    }

    let state = server::AppState {
        sync_dir: sync_dir.clone(),
        file_events,
        identity_short: identity.id_short(),
        identity_id: identity.id,
        device_id,
        capture_engine,
        event_log,
        app_registry,
        operation_context,
        account_profiles,
        credential_vault,
        vault_sessions,
        email_sync,
        query_planner,
        verification_codes: Arc::new(Mutex::new(bridge::VerificationCodeInbox::default())),
        devices: Arc::new(Mutex::new(server::DeviceRegistry::default())),
        connect_urls: connect_urls.clone(),
        relay_url,
        relay_space,
        device_name,
    };

    let app = server::create_router(state);
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().expect("invalid addr");

    println!("\nAEON Flow capture service started");
    println!("----------------------------------------");
    println!("Identity ID: {}", identity.id_short());
    println!("Sync dir:    {}", sync_dir.display());
    for url in &connect_urls {
        println!("{:12} {}", format!("{}:", url.label), url.url);
    }
    println!("----------------------------------------");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("server failed");
}

async fn spawn_embedded_relay(config: RelayServeConfig) {
    std::fs::create_dir_all(&config.dir).expect("failed to create relay directory");
    let store = relay::RelayStore::new(config.dir.clone());
    let app = relay::create_relay_router(store, config.space.clone());
    let addr: SocketAddr = format!("0.0.0.0:{}", config.port)
        .parse()
        .expect("invalid relay addr");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("embedded relay bind failed");

    println!("AEON embedded Relay started");
    println!("Relay space: {}", config.space);
    println!("Relay store: {}", config.dir.display());
    println!("Relay URL:   http://0.0.0.0:{}", config.port);

    tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("embedded relay server failed");
    });
}

async fn run_relay_mode(args: Vec<String>) {
    let config = parse_relay_args(args);
    std::fs::create_dir_all(&config.dir).expect("failed to create relay directory");
    let store = relay::RelayStore::new(config.dir.clone());
    let app = relay::create_relay_router(store, config.space.clone());
    let addr: SocketAddr = format!("0.0.0.0:{}", config.port)
        .parse()
        .expect("invalid relay addr");

    println!("\nAEON Relay started");
    println!("----------------------------------------");
    println!("Space: {}", config.space);
    println!("Store: {}", config.dir.display());
    println!("URL:   http://0.0.0.0:{}", config.port);
    println!("----------------------------------------");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("relay bind failed");
    axum::serve(listener, app)
        .await
        .expect("relay server failed");
}

#[derive(Clone)]
struct RelayServeConfig {
    port: u16,
    dir: std::path::PathBuf,
    space: String,
}

struct StartConfig {
    port: u16,
    with_relay: bool,
    relay: RelayServeConfig,
    relay_url: Option<String>,
    discovery_port: u16,
}

#[derive(Clone)]
struct LanDiscoveryConfig {
    port: u16,
    ui_port: u16,
    relay_port: Option<u16>,
    device_id: String,
    device_name: String,
    identity_short: String,
}

fn default_relay_config() -> RelayServeConfig {
    let home = dirs::home_dir().expect("Cannot find home directory");
    RelayServeConfig {
        port: std::env::var("AEON_RELAY_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8090),
        dir: std::env::var("AEON_RELAY_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| home.join(".aeon-relay")),
        space: std::env::var("AEON_RELAY_SPACE").unwrap_or_else(|_| "default".to_string()),
    }
}

fn parse_start_args(args: Vec<String>) -> StartConfig {
    let mut config = StartConfig {
        port: std::env::var("AEON_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8080),
        with_relay: env_flag("AEON_WITH_RELAY").unwrap_or(true),
        relay: default_relay_config(),
        relay_url: std::env::var("AEON_RELAY_URL")
            .ok()
            .and_then(|raw| normalize_remote_url(&raw)),
        discovery_port: std::env::var("AEON_DISCOVERY_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(8091),
    };

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                if let Some(value) = iter.next() {
                    config.port = value.parse().expect("invalid port");
                }
            }
            "--with-relay" => {
                config.with_relay = true;
            }
            "--no-relay" => {
                config.with_relay = false;
            }
            "--relay-port" => {
                if let Some(value) = iter.next() {
                    config.relay.port = value.parse().expect("invalid relay port");
                }
            }
            "--relay-dir" => {
                if let Some(value) = iter.next() {
                    config.relay.dir = value.into();
                }
            }
            "--space" | "--relay-space" => {
                if let Some(value) = iter.next() {
                    config.relay.space = value;
                }
            }
            "--relay-url" => {
                if let Some(value) = iter.next() {
                    config.relay_url = normalize_remote_url(&value);
                }
            }
            "--discovery-port" => {
                if let Some(value) = iter.next() {
                    config.discovery_port = value.parse().expect("invalid discovery port");
                }
            }
            "--help" | "-h" => {
                print_start_usage();
                std::process::exit(0);
            }
            _ => {}
        }
    }

    config
}

fn parse_relay_args(args: Vec<String>) -> RelayServeConfig {
    let mut config = default_relay_config();

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--port" | "-p" => {
                if let Some(value) = iter.next() {
                    config.port = value.parse().expect("invalid relay port");
                }
            }
            "--dir" => {
                if let Some(value) = iter.next() {
                    config.dir = value.into();
                }
            }
            "--space" => {
                if let Some(value) = iter.next() {
                    config.space = value;
                }
            }
            "--help" | "-h" => {
                println!("Usage: aeon-sync relay [--port 8090] [--dir PATH] [--space NAME]");
                std::process::exit(0);
            }
            _ => {}
        }
    }

    config
}

fn env_flag(key: &str) -> Option<bool> {
    let value = std::env::var(key).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn print_start_usage() {
    println!(
        "Usage: aeon-sync [start] [--port 8080] [--with-relay|--no-relay] [--relay-port 8090] [--relay-dir PATH] [--relay-space NAME] [--relay-url URL] [--discovery-port 8091]"
    );
}

fn connection_urls(
    port: u16,
    embedded_relay_port: Option<u16>,
    relay_url: Option<&str>,
) -> Vec<server::ConnectUrl> {
    let mut urls = Vec::new();
    let mut seen = HashSet::new();
    let lan_ips = local_ip_addresses();
    let show_tailscale = env_flag("AEON_SHOW_TAILSCALE").unwrap_or(false);
    let tailscale_ip = show_tailscale.then(tailscale_ip_address).flatten();
    push_connect_url(
        &mut urls,
        &mut seen,
        "local",
        "Local",
        format!("http://localhost:{port}"),
        "local",
        false,
    );

    for (index, ip) in lan_ips.iter().enumerate() {
        let label = if index == 0 {
            "LAN".to_string()
        } else {
            format!("LAN {}", index + 1)
        };
        push_connect_url(
            &mut urls,
            &mut seen,
            &format!("lan-{}", index + 1),
            &label,
            format!("http://{ip}:{port}"),
            "lan",
            false,
        );
    }

    if show_tailscale {
        if let Some(ip) = tailscale_ip.as_deref() {
            push_connect_url(
                &mut urls,
                &mut seen,
                "tailscale",
                "Tailscale",
                format!("http://{ip}:{port}"),
                "tailscale",
                true,
            );
        }
    }

    if let Some(relay_port) = embedded_relay_port {
        push_connect_url(
            &mut urls,
            &mut seen,
            "relay-local",
            "AEON Relay Local",
            format!("http://localhost:{relay_port}"),
            "relay",
            true,
        );
        for (index, ip) in lan_ips.iter().enumerate() {
            let label = if index == 0 {
                "AEON Relay LAN".to_string()
            } else {
                format!("AEON Relay LAN {}", index + 1)
            };
            push_connect_url(
                &mut urls,
                &mut seen,
                &format!("relay-lan-{}", index + 1),
                &label,
                format!("http://{ip}:{relay_port}"),
                "relay",
                true,
            );
        }
        if show_tailscale {
            if let Some(ip) = tailscale_ip.as_deref() {
                push_connect_url(
                    &mut urls,
                    &mut seen,
                    "relay-tailscale",
                    "AEON Relay Tailscale",
                    format!("http://{ip}:{relay_port}"),
                    "relay",
                    true,
                );
            }
        }
    }

    if let Some(url) = relay_url {
        push_connect_url(
            &mut urls,
            &mut seen,
            "relay",
            "AEON Relay",
            url.to_string(),
            "relay",
            true,
        );
    }

    for key in ["AEON_PUBLIC_URL", "AEON_REMOTE_URL"] {
        if let Ok(raw) = std::env::var(key) {
            if let Some(url) = normalize_base_url(&raw, port) {
                push_connect_url(
                    &mut urls, &mut seen, "public", "Public", url, "public", true,
                );
                break;
            }
        }
    }

    urls
}

fn spawn_lan_discovery(config: LanDiscoveryConfig) {
    tokio::spawn(async move {
        let addr = format!("0.0.0.0:{}", config.port);
        let socket = match tokio::net::UdpSocket::bind(&addr).await {
            Ok(socket) => socket,
            Err(err) => {
                eprintln!("AEON LAN discovery bind failed on {addr}: {err}");
                return;
            }
        };
        let _ = socket.set_broadcast(true);
        println!(
            "AEON LAN discovery listening on udp://0.0.0.0:{}",
            config.port
        );

        let mut buf = [0u8; 2048];
        loop {
            let Ok((len, peer)) = socket.recv_from(&mut buf).await else {
                continue;
            };
            let message = String::from_utf8_lossy(&buf[..len]);
            if !message.trim().starts_with("AEON_DISCOVER_V1") {
                continue;
            }

            let host = local_ip_for_peer(peer.ip())
                .or_else(local_ip_address)
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let ui_url = format!("http://{}:{}", host, config.ui_port);
            let relay_url = config
                .relay_port
                .map(|port| format!("http://{host}:{port}"));
            let payload = serde_json::json!({
                "ok": true,
                "kind": "aeon-discovery",
                "version": 1,
                "device_id": config.device_id,
                "device_name": config.device_name,
                "identity_short": config.identity_short,
                "preferred_endpoint": ui_url,
                "ui_url": ui_url,
                "relay_url": relay_url,
                "discovery_port": config.port,
            });
            let _ = socket.send_to(payload.to_string().as_bytes(), peer).await;
        }
    });
}

fn local_ip_for_peer(peer: std::net::IpAddr) -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect((peer, 9)).ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn push_connect_url(
    urls: &mut Vec<server::ConnectUrl>,
    seen: &mut HashSet<String>,
    id: &str,
    label: &str,
    url: String,
    kind: &str,
    remote: bool,
) {
    let url = url.trim_end_matches('/').to_string();
    if seen.insert(url.clone()) {
        urls.push(server::ConnectUrl {
            id: id.to_string(),
            label: label.to_string(),
            url,
            kind: kind.to_string(),
            remote,
        });
    }
}

fn normalize_base_url(raw: &str, port: u16) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }
    Some(format!("http://{}:{port}", trimmed.trim_end_matches('/')))
}

fn normalize_remote_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        return Some(trimmed.to_string());
    }
    Some(format!("http://{trimmed}"))
}

fn local_ip_address() -> Option<String> {
    if let Some(ip) = local_ip_addresses().into_iter().next() {
        return Some(ip);
    }

    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn local_ip_addresses() -> Vec<String> {
    let mut ips = Vec::new();

    if let Ok(raw) = std::env::var("AEON_LAN_IPS") {
        ips.extend(
            raw.split([',', ';', ' '])
                .map(str::trim)
                .filter(|ip| !ip.is_empty())
                .map(ToOwned::to_owned),
        );
    }

    #[cfg(target_os = "windows")]
    {
        ips.extend(windows_lan_ip_addresses());
    }
    #[cfg(target_os = "windows")]
    {
        ips.extend(ipconfig_lan_ip_addresses());
    }

    if ips.is_empty() {
        #[cfg(not(target_os = "windows"))]
        if let Some(ip) = udp_default_lan_ip_address() {
            ips.push(ip);
        }
    }

    let mut seen = HashSet::new();
    ips.retain(|ip| seen.insert(ip.clone()));
    ips
}

#[cfg(not(target_os = "windows"))]
fn udp_default_lan_ip_address() -> Option<String> {
    use std::net::UdpSocket;
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

fn tailscale_ip_address() -> Option<String> {
    if let Some(ip) = tailscale_cli_ip_address() {
        return Some(ip);
    }
    #[cfg(target_os = "windows")]
    if let Some(ip) = windows_tailscale_ip_address() {
        return Some(ip);
    }
    None
}

fn tailscale_cli_ip_address() -> Option<String> {
    let output = std::process::Command::new("tailscale")
        .args(["ip", "-4"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter_map(|line| line.parse::<std::net::Ipv4Addr>().ok())
        .find(is_tailscale_ip)
        .map(|ip| ip.to_string())
}

fn is_tailscale_ip(ip: &std::net::Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 100 && (64..=127).contains(&b)
}

#[cfg(target_os = "windows")]
fn windows_lan_ip_addresses() -> Vec<String> {
    let script = r#"
Get-NetIPAddress -AddressFamily IPv4 |
  Where-Object {
    $_.AddressState -eq 'Preferred' -and
    $_.IPAddress -notlike '127.*' -and
    $_.InterfaceAlias -notmatch 'VMware|Virtual|vEthernet|Tailscale|Meta|Loopback|Docker|WSL'
  } |
  Sort-Object @{Expression={ if ($_.InterfaceAlias -match 'WLAN|Wi-Fi') { 0 } elseif ($_.InterfaceAlias -match 'Ethernet|以太网') { 1 } else { 2 } }}, InterfaceAlias |
  Select-Object -ExpandProperty IPAddress
"#;
    let Ok(output) = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_tailscale_ip_address() -> Option<String> {
    let script = r#"
Get-NetIPAddress -AddressFamily IPv4 |
  Where-Object {
    $_.AddressState -eq 'Preferred' -and
    $_.InterfaceAlias -match 'Tailscale'
  } |
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
        .filter(|line| !line.is_empty())
        .find(|line| {
            line.parse::<std::net::Ipv4Addr>()
                .ok()
                .is_some_and(|ip| is_tailscale_ip(&ip))
        })
        .map(ToOwned::to_owned)
}

#[cfg(target_os = "windows")]
fn ipconfig_lan_ip_addresses() -> Vec<String> {
    let Ok(output) = std::process::Command::new("ipconfig").output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut adapter = String::new();
    let mut candidates: Vec<(u8, std::net::Ipv4Addr)> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.to_ascii_lowercase().contains("adapter") && trimmed.ends_with(':') {
            adapter = trimmed.to_string();
            continue;
        }
        if !trimmed.contains("IPv4") || is_ignored_adapter(&adapter) {
            continue;
        }
        if let Some(ip) = extract_ipv4(trimmed) {
            candidates.push((adapter_score(&adapter) + ipv4_score(&ip), ip));
        }
    }

    candidates.sort_by_key(|(score, _)| *score);
    candidates
        .into_iter()
        .map(|(_, ip)| ip.to_string())
        .collect()
}

#[cfg(target_os = "windows")]
fn is_ignored_adapter(adapter: &str) -> bool {
    adapter.contains("VMware")
        || adapter.contains("Virtual")
        || adapter.contains("vEthernet")
        || adapter.contains("Tailscale")
        || adapter.contains("Meta")
        || adapter.contains("Loopback")
        || adapter.contains("Docker")
        || adapter.contains("WSL")
}

#[cfg(target_os = "windows")]
fn adapter_score(adapter: &str) -> u8 {
    if adapter.contains("WLAN") || adapter.contains("Wi-Fi") || adapter.contains("Wireless") {
        0
    } else if adapter.contains("Ethernet") {
        10
    } else {
        20
    }
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

fn local_device_name() -> String {
    std::env::var("AEON_DEVICE_NAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "AEON Desktop".to_string())
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
