use mdns_sd::{ServiceDaemon, ServiceInfo};

pub struct Discovery {
    pub identity_id: [u8; 32],
    pub port: u16,
}

impl Discovery {
    pub fn announce(&self) -> Result<ServiceDaemon, String> {
        let mdns = ServiceDaemon::new().map_err(|e| e.to_string())?;
        let short = hex::encode(&self.identity_id[..4]);
        let instance = format!("aeon-{short}");
        let host = format!("aeon-{short}.local.");
        let service = ServiceInfo::new("_aeon._tcp.local.", &instance, &host, "", self.port, None)
            .map_err(|e| e.to_string())?;
        mdns.register(service).map_err(|e| e.to_string())?;
        Ok(mdns)
    }
}
