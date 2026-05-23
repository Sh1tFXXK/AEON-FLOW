use crate::server::AppState;
use aeon_capture::bridge::{
    BridgePayloadError, BrowserPageBridgePayload, EmailBridgePayload, SmsBridgePayload,
};
use aeon_capture::hex_cid;
use axum::extract::{ConnectInfo, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::net::SocketAddr;

const VERIFICATION_CODE_TTL_MS: u64 = 5 * 60 * 1000;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BridgeCaptureResponse {
    pub ok: bool,
    pub cid: String,
    pub verification_code: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct VerificationCodeInbox {
    latest: Option<VerificationCodeCandidate>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VerificationCodeCandidate {
    pub code: String,
    pub address: String,
    pub received_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LatestVerificationCodeResponse {
    pub code: Option<VerificationCodeCandidate>,
}

impl VerificationCodeInbox {
    pub fn record_sms_code(&mut self, code: String, address: String, received_at: u64) {
        self.latest = Some(VerificationCodeCandidate {
            code,
            address,
            received_at,
            expires_at: received_at.saturating_add(VERIFICATION_CODE_TTL_MS),
        });
    }

    pub fn latest(&self, now_ms: u64) -> Option<VerificationCodeCandidate> {
        self.latest
            .as_ref()
            .filter(|candidate| candidate.expires_at >= now_ms)
            .cloned()
    }
}

pub async fn capture_sms(
    State(state): State<AppState>,
    Json(payload): Json<SmsBridgePayload>,
) -> Result<Json<BridgeCaptureResponse>, StatusCode> {
    let mut entry = payload.into_capture_entry();
    stamp_capture_identity(&mut entry, &state);
    let verification_code = entry.meta.extra.get("verification_code").cloned();
    let verification_address = entry.meta.extra.get("address").cloned();
    let received_at = entry.captured_at;
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let (Some(code), Some(address)) = (verification_code.clone(), verification_address) {
        state
            .verification_codes
            .lock()
            .await
            .record_sms_code(code, address, received_at);
    }

    Ok(Json(BridgeCaptureResponse {
        ok: true,
        cid: hex_cid(&cid),
        verification_code,
    }))
}

pub async fn latest_verification_code(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Result<Json<LatestVerificationCodeResponse>, StatusCode> {
    if !addr.ip().is_loopback() {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(Json(LatestVerificationCodeResponse {
        code: state.verification_codes.lock().await.latest(now_ms()),
    }))
}

pub async fn capture_email(
    State(state): State<AppState>,
    Json(payload): Json<EmailBridgePayload>,
) -> Result<Json<BridgeCaptureResponse>, StatusCode> {
    let mut entry = payload.into_capture_entry();
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(BridgeCaptureResponse {
        ok: true,
        cid: hex_cid(&cid),
        verification_code: None,
    }))
}

pub async fn capture_browser_page(
    State(state): State<AppState>,
    Json(payload): Json<BrowserPageBridgePayload>,
) -> Result<Json<BridgeCaptureResponse>, StatusCode> {
    let mut entry = payload
        .into_capture_entry()
        .map_err(bridge_payload_status)?;
    stamp_capture_identity(&mut entry, &state);
    let cid = state
        .capture_engine
        .capture(entry)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(BridgeCaptureResponse {
        ok: true,
        cid: hex_cid(&cid),
        verification_code: None,
    }))
}

fn bridge_payload_status(error: BridgePayloadError) -> StatusCode {
    match error {
        BridgePayloadError::UnsupportedUrl => StatusCode::BAD_REQUEST,
    }
}

fn stamp_capture_identity(entry: &mut aeon_capture::CaptureEntry, state: &AppState) {
    entry.by = state.identity_id;
    entry.device = state.device_id;
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{AppState, DeviceRegistry};
    use aeon_capture::bridge::{EmailBridgePayload, SmsBridgePayload, SmsDirection};
    use aeon_capture::{CaptureEngine, CaptureKind, CaptureStore, EventLog};
    use aeon_store::CIDStore;
    use axum::{extract::State, Json};
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::{broadcast, Mutex};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-sync-bridge-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn test_state(dir: &std::path::Path) -> AppState {
        let event_log = Arc::new(Mutex::new(EventLog::new(dir.join("events.jsonl"))));
        let store = CaptureStore::new(
            CIDStore::new(dir.join("store")).unwrap(),
            dir.join("capture-index.json"),
        )
        .unwrap();
        let engine = Arc::new(CaptureEngine::new_with_identity_and_events(
            Arc::new(Mutex::new(store)),
            [1u8; 32],
            [2u8; 16],
            Some(event_log.clone()),
        ));
        let (file_events, _) = broadcast::channel(8);

        AppState {
            sync_dir: dir.join("sync"),
            file_events,
            identity_short: "test".to_string(),
            identity_id: [1u8; 32],
            device_id: [2u8; 16],
            capture_engine: engine.clone(),
            event_log,
            app_registry: Arc::new(aeon_capture::apps::default_registry(engine)),
            operation_context: Arc::new(Mutex::new(
                crate::operation_context::ContextStore::new(dir.join("context.json")).unwrap(),
            )),
            account_profiles: Arc::new(Mutex::new(
                crate::account_profiles::AccountProfileStore::new(
                    dir.join("account-profiles.json"),
                )
                .unwrap(),
            )),
            credential_vault: Arc::new(Mutex::new(
                crate::vault::CredentialVaultStore::new(dir.join("vault.json")).unwrap(),
            )),
            vault_sessions: Arc::new(Mutex::new(crate::vault::CredentialUnlockSessions::default())),
            email_sync: Arc::new(Mutex::new(
                crate::email_sync::EmailSyncStore::new(dir.join("email-sync.json")).unwrap(),
            )),
            query_planner: None,
            verification_codes: Arc::new(Mutex::new(VerificationCodeInbox::default())),
            devices: Arc::new(Mutex::new(DeviceRegistry::default())),
            connect_urls: Vec::new(),
            relay_url: None,
            relay_space: "test".to_string(),
            device_name: "Test Device".to_string(),
        }
    }

    #[tokio::test]
    async fn sms_bridge_handler_captures_payload_and_returns_code() {
        let dir = temp_dir();
        let state = test_state(&dir);
        let response = capture_sms(
            State(state.clone()),
            Json(SmsBridgePayload {
                message_id: "sms-1".to_string(),
                address: "10086".to_string(),
                body: "您的验证码是 476291，5分钟内有效".to_string(),
                received_at: 1_771_000_000_000,
                direction: SmsDirection::Incoming,
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.verification_code.as_deref(), Some("476291"));

        let records = state.capture_engine.list().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, CaptureKind::Text);
        assert_eq!(
            records[0].meta.extra.get("bridge.kind").map(String::as_str),
            Some("sms")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn latest_verification_code_returns_recent_sms_code() {
        let dir = temp_dir();
        let state = test_state(&dir);
        let _ = capture_sms(
            State(state.clone()),
            Json(SmsBridgePayload {
                message_id: "sms-code".to_string(),
                address: "10086".to_string(),
                body: "verification code: 476291".to_string(),
                received_at: now_ms(),
                direction: SmsDirection::Incoming,
            }),
        )
        .await
        .unwrap();

        let response = latest_verification_code(loopback_peer(), State(state))
            .await
            .unwrap()
            .0;

        assert_eq!(response.code.unwrap().code, "476291");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn latest_verification_code_omits_expired_sms_code() {
        let dir = temp_dir();
        let state = test_state(&dir);
        let _ = capture_sms(
            State(state.clone()),
            Json(SmsBridgePayload {
                message_id: "sms-code".to_string(),
                address: "10086".to_string(),
                body: "verification code: 476291".to_string(),
                received_at: now_ms().saturating_sub(VERIFICATION_CODE_TTL_MS + 1),
                direction: SmsDirection::Incoming,
            }),
        )
        .await
        .unwrap();

        let response = latest_verification_code(loopback_peer(), State(state))
            .await
            .unwrap()
            .0;

        assert!(response.code.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn latest_verification_code_rejects_non_loopback_clients() {
        let dir = temp_dir();
        let state = test_state(&dir);
        let result = latest_verification_code(
            ConnectInfo(SocketAddr::from(([192, 168, 1, 44], 49152))),
            State(state),
        )
        .await;

        assert_eq!(result.unwrap_err(), StatusCode::FORBIDDEN);
        let _ = std::fs::remove_dir_all(dir);
    }

    fn loopback_peer() -> ConnectInfo<SocketAddr> {
        ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 49152)))
    }

    #[tokio::test]
    async fn email_bridge_handler_captures_payload() {
        let dir = temp_dir();
        let state = test_state(&dir);
        let response = capture_email(
            State(state.clone()),
            Json(EmailBridgePayload {
                message_id: "email-1".to_string(),
                from: "noreply@example.test".to_string(),
                to: vec!["wc@example.test".to_string()],
                subject: "Build finished".to_string(),
                body_preview: "AEON build completed successfully".to_string(),
                received_at: 1_771_000_000_100,
                labels: vec!["inbox".to_string()],
            }),
        )
        .await
        .unwrap()
        .0;

        assert!(response.verification_code.is_none());

        let records = state.capture_engine.list().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].meta.title.as_deref(), Some("Build finished"));
        assert_eq!(
            records[0].meta.extra.get("bridge.kind").map(String::as_str),
            Some("email")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn browser_page_bridge_handler_captures_live_tab_fact() {
        let dir = temp_dir();
        let state = test_state(&dir);
        let response = capture_browser_page(
            State(state.clone()),
            Json(BrowserPageBridgePayload {
                url: "https://example.test/private".to_string(),
                title: "Private dashboard".to_string(),
                captured_at: 1_771_000_000_222,
                account_id: Some("google-work".to_string()),
                tab_id: Some(42),
            }),
        )
        .await
        .unwrap()
        .0;

        assert!(response.ok);
        assert!(response.verification_code.is_none());

        let records = state.capture_engine.list().await;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, CaptureKind::Webpage);
        assert_eq!(
            records[0].meta.extra.get("bridge.kind").map(String::as_str),
            Some("browser_page")
        );
        assert_eq!(
            records[0].meta.url.as_deref(),
            Some("https://example.test/private")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn browser_page_bridge_handler_rejects_non_web_urls() {
        let dir = temp_dir();
        let state = test_state(&dir);
        let result = capture_browser_page(
            State(state),
            Json(BrowserPageBridgePayload {
                url: "file:///C:/Users/Wc/secret.txt".to_string(),
                title: "secret".to_string(),
                captured_at: 1_771_000_000_222,
                account_id: None,
                tab_id: None,
            }),
        )
        .await;

        assert_eq!(result.unwrap_err(), StatusCode::BAD_REQUEST);
        let _ = std::fs::remove_dir_all(dir);
    }
}
