use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    pub provider: String,
    pub endpoint: String,
    pub health_interval_secs: u64,
}

impl TunnelConfig {
    pub fn from_env() -> Option<Self> {
        let endpoint = std::env::var("AEON_TUNNEL_ENDPOINT").ok()?.trim().to_string();
        if endpoint.is_empty() { return None; }
        let provider = std::env::var("AEON_TUNNEL_PROVIDER").unwrap_or_else(|_| "cloudflare".to_string());
        let health_interval_secs = std::env::var("AEON_TUNNEL_HEALTH_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
        Some(Self { provider, endpoint, health_interval_secs })
    }
}

pub async fn health_check(endpoint: &str) -> bool {
    tokio::time::timeout(std::time::Duration::from_secs(2), tokio::net::TcpStream::connect(endpoint)).await.is_ok()
}
