use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

fn mdns_enabled_by_default() -> bool {
    // On Windows/Linux, virtual adapters (VPN/Tailscale/etc.) frequently reject
    // multicast packets and cause noisy runtime errors from the mDNS loop.
    // Keep mDNS opt-in there unless explicitly enabled.
    !(cfg!(target_os = "windows") || cfg!(target_os = "linux"))
}

pub struct Discovery {
    pub identity_id: [u8; 32],
    pub port: u16,
}

impl Discovery {
    pub fn try_announce(&self) -> Option<ServiceDaemon> {
        if std::env::var("AEON_AGENT_DISABLE_MDNS").ok().as_deref() == Some("1") {
            tracing::warn!("mDNS discovery disabled via AEON_AGENT_DISABLE_MDNS=1");
            return None;
        }
        if std::env::var("AEON_AGENT_ENABLE_MDNS").ok().as_deref() == Some("1") {
            tracing::info!("mDNS discovery forced on via AEON_AGENT_ENABLE_MDNS=1");
        } else if !mdns_enabled_by_default() {
            tracing::warn!(
                "mDNS disabled by default on this platform; set AEON_AGENT_ENABLE_MDNS=1 to enable"
            );
            return None;
        }

        let mdns = match ServiceDaemon::new() {
            Ok(mdns) => mdns,
            Err(err) => {
                tracing::warn!(error = %err, "mDNS unavailable; continuing without local discovery");
                return None;
            }
        };

        let short = hex::encode(&self.identity_id[..4]);
        let instance = format!("aeon-{short}");
        let host = format!("aeon-{short}.local.");
        let service =
            match ServiceInfo::new("_aeon._tcp.local.", &instance, &host, "", self.port, None) {
                Ok(service) => service,
                Err(err) => {
                    tracing::warn!(error = %err, "failed to build mDNS service info");
                    return None;
                }
            };

        if let Err(err) = mdns.register(service) {
            tracing::warn!(error = %err, "failed to register mDNS service; continuing without local discovery");
            return None;
        }

        Some(mdns)
    }

    pub fn browse(mdns: &ServiceDaemon) -> Result<mdns_sd::Receiver<ServiceEvent>, String> {
        mdns.browse("_aeon._tcp.local.").map_err(|e| e.to_string())
    }
}
