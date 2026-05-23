use super::*;

use super::shared::*;

#[derive(Serialize)]
pub struct StatusPayload {
    pub identity_short: String,
    pub devices: Vec<DeviceStatus>,
    pub connect_urls: Vec<ConnectUrl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectUrl {
    pub id: String,
    pub label: String,
    pub url: String,
    pub kind: String,
    pub remote: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceStatus {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub kind: String,
    pub endpoint: Option<String>,
    pub last_seen_ms: Option<u64>,
    pub is_local: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDevice {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub endpoint: Option<String>,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default)]
pub struct DeviceRegistry {
    peers: HashMap<String, PeerDevice>,
}

#[derive(Debug, Deserialize)]
pub struct DeviceHelloPayload {
    pub id: Option<String>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub endpoint: Option<String>,
}

impl DeviceRegistry {
    pub fn upsert(&mut self, device: PeerDevice) {
        self.peers.insert(device.id.clone(), device);
    }

    pub fn list(&mut self, now: u64) -> Vec<DeviceStatus> {
        self.peers
            .retain(|_, peer| now.saturating_sub(peer.last_seen) <= DEVICE_KEEP_OFFLINE_MS);

        let mut devices: Vec<_> = self
            .peers
            .values()
            .map(|peer| {
                let age = now.saturating_sub(peer.last_seen);
                DeviceStatus {
                    id: peer.id.clone(),
                    name: peer.name.clone(),
                    online: age <= DEVICE_ONLINE_TTL_MS,
                    kind: peer.kind.clone(),
                    endpoint: peer.endpoint.clone(),
                    last_seen_ms: Some(age),
                    is_local: false,
                }
            })
            .collect();
        devices.sort_by(|a, b| {
            b.online
                .cmp(&a.online)
                .then(a.kind.cmp(&b.kind))
                .then(a.name.cmp(&b.name))
        });
        devices
    }
}

pub async fn status(State(state): State<AppState>) -> Json<StatusPayload> {
    let mut devices = vec![DeviceStatus {
        id: "local".to_string(),
        name: state.device_name.clone(),
        online: true,
        kind: "desktop".to_string(),
        endpoint: None,
        last_seen_ms: Some(0),
        is_local: true,
    }];
    devices.extend(state.devices.lock().await.list(now_ms()));

    Json(StatusPayload {
        identity_short: state.identity_short,
        devices,
        connect_urls: state.connect_urls.clone(),
    })
}

pub async fn device_hello(
    State(state): State<AppState>,
    Json(payload): Json<DeviceHelloPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let Some(id) = payload
        .id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty() && *id != "local")
        .map(ToOwned::to_owned)
    else {
        return Err(StatusCode::BAD_REQUEST);
    };

    let name = payload
        .name
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Unnamed device")
        .to_string();
    let kind = payload
        .kind
        .as_deref()
        .map(str::trim)
        .filter(|kind| !kind.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let endpoint = payload
        .endpoint
        .map(|endpoint| endpoint.trim().trim_end_matches('/').to_string())
        .filter(|endpoint| !endpoint.is_empty());

    state.devices.lock().await.upsert(PeerDevice {
        id: id.clone(),
        name,
        kind,
        endpoint,
        last_seen: now_ms(),
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "id": id
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_registry_reports_online_and_offline_peers() {
        let now = 1_000_000;
        let mut registry = DeviceRegistry::default();
        registry.upsert(PeerDevice {
            id: "android-1".to_string(),
            name: "Android Phone".to_string(),
            kind: "android".to_string(),
            endpoint: None,
            last_seen: now - 1_000,
        });
        registry.upsert(PeerDevice {
            id: "tablet-1".to_string(),
            name: "Tablet".to_string(),
            kind: "android".to_string(),
            endpoint: None,
            last_seen: now - DEVICE_ONLINE_TTL_MS - 1,
        });

        let devices = registry.list(now);
        let phone = devices.iter().find(|d| d.id == "android-1").unwrap();
        let tablet = devices.iter().find(|d| d.id == "tablet-1").unwrap();

        assert!(phone.online);
        assert!(!phone.is_local);
        assert!(!tablet.online);
    }

    #[test]
    fn device_registry_drops_very_old_peers() {
        let now = 1_000_000;
        let mut registry = DeviceRegistry::default();
        registry.upsert(PeerDevice {
            id: "old-phone".to_string(),
            name: "Old Phone".to_string(),
            kind: "android".to_string(),
            endpoint: None,
            last_seen: now - DEVICE_KEEP_OFFLINE_MS - 1,
        });

        assert!(registry.list(now).is_empty());
    }
}
