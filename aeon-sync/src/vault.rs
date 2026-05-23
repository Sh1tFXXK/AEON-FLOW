use crate::server::AppState;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::extract::{ConnectInfo, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::Json;
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::Sha256;
use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const DEFAULT_ITERATIONS: u32 = 120_000;
const TOTP_PERIOD_SECS: u64 = 30;
const TOTP_DIGITS: u32 = 6;
const DEFAULT_UNLOCK_TTL_MS: u64 = 5 * 60_000;
const MAX_UNLOCK_TTL_MS: u64 = 30 * 60_000;
const MILLIS_PER_SECOND: u64 = 1_000;
const OAUTH_REFRESH_SKEW_MS: u64 = 60_000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialEntryInput {
    pub id: String,
    pub kind: CredentialKind,
    pub label: String,
    pub domains: Vec<String>,
    pub last_used: u64,
    pub auto_fill: bool,
    pub secret: CredentialSecret,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CredentialMetadata {
    pub id: String,
    pub kind: CredentialKind,
    pub label: String,
    pub domains: Vec<String>,
    pub last_used: u64,
    pub auto_fill: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TotpCode {
    pub code: String,
    pub expires_at: u64,
    pub period: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CredentialKind {
    Password,
    OAuthToken,
    BrowserSession,
    ApiKey,
    SshKey,
    Totp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CredentialSecret {
    Password {
        username: String,
        password: String,
    },
    OAuthToken {
        access_token: String,
        refresh_token: String,
        expires_at: u64,
        scopes: Vec<String>,
        #[serde(default)]
        token_url: Option<String>,
        #[serde(default)]
        client_id: Option<String>,
        #[serde(default)]
        client_secret: Option<String>,
    },
    BrowserSession {
        cookies_json: String,
        user_agent: String,
    },
    ApiKey {
        key: String,
        header_name: String,
    },
    SshKey {
        public_key: String,
        private_key: String,
    },
    Totp {
        secret: String,
        issuer: String,
    },
}

#[derive(Debug, Deserialize)]
pub struct AddCredentialPayload {
    pub password: String,
    pub entry: CredentialEntryInput,
}

#[derive(Debug, Deserialize)]
pub struct TotpCodePayload {
    pub password: String,
    pub now: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct UnlockVaultPayload {
    pub password: String,
    pub ttl_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct UnlockVaultResponse {
    pub session_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Deserialize)]
pub struct CredentialFillPayload {
    pub session_id: String,
    pub url: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PasswordFillCredential {
    pub id: String,
    pub label: String,
    pub username: String,
    pub password: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordCredential {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CredentialFillResponse {
    pub credential: Option<PasswordFillCredential>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthRefreshRequest {
    pub url: String,
    pub form: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthAccessTokenState {
    Ready(String),
    NeedsRefresh {
        request: OAuthRefreshRequest,
        current: CredentialSecret,
    },
}

#[derive(Debug)]
pub struct CredentialVaultStore {
    path: PathBuf,
    file: VaultFile,
}

#[derive(Debug, Default)]
pub struct CredentialUnlockSessions {
    sessions: HashMap<String, CredentialUnlockSession>,
}

#[derive(Debug, Clone)]
struct CredentialUnlockSession {
    key: [u8; KEY_LEN],
    expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VaultFile {
    salt: Vec<u8>,
    iterations: u32,
    entries: Vec<EncryptedCredentialEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedCredentialEntry {
    metadata: CredentialMetadata,
    nonce: Vec<u8>,
    encrypted_data: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VaultError {
    Io,
    Serialize,
    CryptoUnavailable,
    MissingEntry,
    DecryptFailed,
    UnsupportedCredentialKind,
    InvalidTotpSecret,
    InvalidUrl,
    MissingOAuthRefreshConfig,
    InvalidOAuthResponse,
}

impl CredentialVaultStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, VaultError> {
        let path = path.as_ref().to_path_buf();
        let file = read_vault_file(&path)?;
        Ok(Self { path, file })
    }

    pub fn list_metadata(&self) -> Vec<CredentialMetadata> {
        self.file
            .entries
            .iter()
            .map(|entry| entry.metadata.clone())
            .collect()
    }

    pub fn add_entry(
        &mut self,
        password: &str,
        input: CredentialEntryInput,
    ) -> Result<(), VaultError> {
        let key = derive_key(password, &self.file.salt, self.file.iterations);
        let nonce = random_bytes(NONCE_LEN);
        let secret_bytes = serde_json::to_vec(&input.secret).map_err(|_| VaultError::Serialize)?;
        let encrypted_data = encrypt(&key, &nonce, &secret_bytes)?;
        let entry = EncryptedCredentialEntry {
            metadata: CredentialMetadata {
                id: input.id,
                kind: input.kind,
                label: input.label,
                domains: input.domains,
                last_used: input.last_used,
                auto_fill: input.auto_fill,
            },
            nonce,
            encrypted_data,
        };

        if let Some(existing) = self
            .file
            .entries
            .iter_mut()
            .find(|existing| existing.metadata.id == entry.metadata.id)
        {
            *existing = entry;
        } else {
            self.file.entries.push(entry);
        }
        self.file
            .entries
            .sort_by(|a, b| a.metadata.id.cmp(&b.metadata.id));
        write_vault_file(&self.path, &self.file)
    }

    pub fn decrypt_entry(
        &self,
        password: &str,
        entry_id: &str,
    ) -> Result<CredentialSecret, VaultError> {
        let entry = self
            .file
            .entries
            .iter()
            .find(|entry| entry.metadata.id == entry_id)
            .ok_or(VaultError::MissingEntry)?;
        let key = derive_key(password, &self.file.salt, self.file.iterations);
        decrypt_entry_with_key(entry, &key)
    }

    pub fn unlock_key(&self, password: &str) -> Result<[u8; KEY_LEN], VaultError> {
        let key = derive_key(password, &self.file.salt, self.file.iterations);
        if let Some(entry) = self.file.entries.first() {
            let _ = decrypt_entry_with_key(entry, &key)?;
        }
        Ok(key)
    }

    pub fn password_fill_for_url(
        &self,
        key: &[u8; KEY_LEN],
        url: &str,
        expires_at: u64,
    ) -> Result<Option<PasswordFillCredential>, VaultError> {
        let host = host_from_http_url(url).ok_or(VaultError::InvalidUrl)?;

        for entry in &self.file.entries {
            if entry.metadata.kind != CredentialKind::Password || !entry.metadata.auto_fill {
                continue;
            }
            if !entry
                .metadata
                .domains
                .iter()
                .any(|domain| domain_matches_host(domain, &host))
            {
                continue;
            }

            let CredentialSecret::Password { username, password } =
                decrypt_entry_with_key(entry, key)?
            else {
                return Err(VaultError::UnsupportedCredentialKind);
            };

            return Ok(Some(PasswordFillCredential {
                id: entry.metadata.id.clone(),
                label: entry.metadata.label.clone(),
                username,
                password,
                expires_at,
            }));
        }

        Ok(None)
    }

    pub fn password_credential_with_key(
        &self,
        key: &[u8; KEY_LEN],
        entry_id: &str,
    ) -> Result<PasswordCredential, VaultError> {
        let entry = self
            .file
            .entries
            .iter()
            .find(|entry| entry.metadata.id == entry_id)
            .ok_or(VaultError::MissingEntry)?;
        if entry.metadata.kind != CredentialKind::Password {
            return Err(VaultError::UnsupportedCredentialKind);
        }
        let CredentialSecret::Password { username, password } = decrypt_entry_with_key(entry, key)?
        else {
            return Err(VaultError::UnsupportedCredentialKind);
        };
        Ok(PasswordCredential { username, password })
    }

    pub fn oauth_access_token_with_key(
        &self,
        key: &[u8; KEY_LEN],
        entry_id: &str,
    ) -> Result<String, VaultError> {
        let entry = self
            .file
            .entries
            .iter()
            .find(|entry| entry.metadata.id == entry_id)
            .ok_or(VaultError::MissingEntry)?;
        if entry.metadata.kind != CredentialKind::OAuthToken {
            return Err(VaultError::UnsupportedCredentialKind);
        }
        let CredentialSecret::OAuthToken { access_token, .. } = decrypt_entry_with_key(entry, key)?
        else {
            return Err(VaultError::UnsupportedCredentialKind);
        };
        Ok(access_token)
    }

    pub fn oauth_access_token_state_with_key(
        &self,
        key: &[u8; KEY_LEN],
        entry_id: &str,
        now_ms: u64,
    ) -> Result<OAuthAccessTokenState, VaultError> {
        let entry = self
            .file
            .entries
            .iter()
            .find(|entry| entry.metadata.id == entry_id)
            .ok_or(VaultError::MissingEntry)?;
        if entry.metadata.kind != CredentialKind::OAuthToken {
            return Err(VaultError::UnsupportedCredentialKind);
        }
        let secret = decrypt_entry_with_key(entry, key)?;
        let CredentialSecret::OAuthToken {
            access_token,
            expires_at,
            ..
        } = &secret
        else {
            return Err(VaultError::UnsupportedCredentialKind);
        };

        if *expires_at > now_ms.saturating_add(OAUTH_REFRESH_SKEW_MS) {
            return Ok(OAuthAccessTokenState::Ready(access_token.clone()));
        }

        Ok(OAuthAccessTokenState::NeedsRefresh {
            request: build_oauth_refresh_request(&secret)?,
            current: secret,
        })
    }

    pub fn replace_secret_with_key(
        &mut self,
        key: &[u8; KEY_LEN],
        entry_id: &str,
        secret: CredentialSecret,
    ) -> Result<(), VaultError> {
        let entry = self
            .file
            .entries
            .iter_mut()
            .find(|entry| entry.metadata.id == entry_id)
            .ok_or(VaultError::MissingEntry)?;
        if entry.metadata.kind != credential_secret_kind(&secret) {
            return Err(VaultError::UnsupportedCredentialKind);
        }

        let nonce = random_bytes(NONCE_LEN);
        let secret_bytes = serde_json::to_vec(&secret).map_err(|_| VaultError::Serialize)?;
        let encrypted_data = encrypt(key, &nonce, &secret_bytes)?;
        entry.nonce = nonce;
        entry.encrypted_data = encrypted_data;
        write_vault_file(&self.path, &self.file)
    }

    pub fn totp_code(
        &self,
        password: &str,
        entry_id: &str,
        now_secs: u64,
    ) -> Result<TotpCode, VaultError> {
        let secret = self.decrypt_entry(password, entry_id)?;
        let CredentialSecret::Totp { secret, .. } = secret else {
            return Err(VaultError::UnsupportedCredentialKind);
        };
        let key = decode_base32_secret(&secret)?;
        let counter = now_secs / TOTP_PERIOD_SECS;
        let code = hotp_sha1(&key, counter, TOTP_DIGITS)?;
        Ok(TotpCode {
            code,
            expires_at: (counter + 1) * TOTP_PERIOD_SECS,
            period: TOTP_PERIOD_SECS,
        })
    }
}

impl CredentialUnlockSessions {
    pub fn unlock(
        &mut self,
        key: [u8; KEY_LEN],
        ttl_ms: Option<u64>,
        now_ms: u64,
    ) -> UnlockVaultResponse {
        self.remove_expired(now_ms);
        let ttl = ttl_ms
            .unwrap_or(DEFAULT_UNLOCK_TTL_MS)
            .clamp(1_000, MAX_UNLOCK_TTL_MS);
        let session_id = hex_bytes(&random_bytes(32));
        let expires_at = now_ms.saturating_add(ttl);
        self.sessions.insert(
            session_id.clone(),
            CredentialUnlockSession { key, expires_at },
        );
        UnlockVaultResponse {
            session_id,
            expires_at,
        }
    }

    pub fn session_key(&mut self, session_id: &str, now_ms: u64) -> Option<([u8; KEY_LEN], u64)> {
        self.remove_expired(now_ms);
        self.sessions
            .get(session_id)
            .map(|session| (session.key, session.expires_at))
    }

    fn remove_expired(&mut self, now_ms: u64) {
        self.sessions
            .retain(|_, session| session.expires_at > now_ms);
    }
}

pub async fn list_entries(State(state): State<AppState>) -> Json<Vec<CredentialMetadata>> {
    Json(state.credential_vault.lock().await.list_metadata())
}

pub async fn add_entry(
    State(state): State<AppState>,
    Json(payload): Json<AddCredentialPayload>,
) -> Result<Json<Vec<CredentialMetadata>>, StatusCode> {
    let mut store = state.credential_vault.lock().await;
    store
        .add_entry(&payload.password, payload.entry)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(store.list_metadata()))
}

pub async fn totp_code(
    AxumPath(entry_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(payload): Json<TotpCodePayload>,
) -> Result<Json<TotpCode>, StatusCode> {
    let now = payload.now.unwrap_or_else(now_secs);
    let store = state.credential_vault.lock().await;
    let code = store
        .totp_code(&payload.password, &entry_id, now)
        .map_err(vault_status)?;
    Ok(Json(code))
}

pub async fn unlock_vault(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<UnlockVaultPayload>,
) -> Result<Json<UnlockVaultResponse>, StatusCode> {
    require_loopback(addr)?;
    let key = state
        .credential_vault
        .lock()
        .await
        .unlock_key(&payload.password)
        .map_err(vault_unlock_status)?;
    let response = state
        .vault_sessions
        .lock()
        .await
        .unlock(key, payload.ttl_ms, now_ms());
    Ok(Json(response))
}

pub async fn credential_fill(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
    Json(payload): Json<CredentialFillPayload>,
) -> Result<Json<CredentialFillResponse>, StatusCode> {
    require_loopback(addr)?;
    let (key, expires_at) = state
        .vault_sessions
        .lock()
        .await
        .session_key(&payload.session_id, now_ms())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let credential = state
        .credential_vault
        .lock()
        .await
        .password_fill_for_url(&key, &payload.url, expires_at)
        .map_err(vault_status)?;
    Ok(Json(CredentialFillResponse { credential }))
}

fn require_loopback(addr: SocketAddr) -> Result<(), StatusCode> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn vault_status(error: VaultError) -> StatusCode {
    match error {
        VaultError::MissingEntry => StatusCode::NOT_FOUND,
        VaultError::DecryptFailed
        | VaultError::UnsupportedCredentialKind
        | VaultError::InvalidTotpSecret
        | VaultError::InvalidUrl
        | VaultError::MissingOAuthRefreshConfig
        | VaultError::InvalidOAuthResponse => StatusCode::BAD_REQUEST,
        VaultError::Io | VaultError::Serialize | VaultError::CryptoUnavailable => {
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

fn vault_unlock_status(error: VaultError) -> StatusCode {
    match error {
        VaultError::DecryptFailed => StatusCode::UNAUTHORIZED,
        other => vault_status(other),
    }
}

fn read_vault_file(path: &Path) -> Result<VaultFile, VaultError> {
    if !path.exists() {
        return Ok(VaultFile {
            salt: random_bytes(SALT_LEN),
            iterations: DEFAULT_ITERATIONS,
            entries: Vec::new(),
        });
    }
    let bytes = std::fs::read(path).map_err(|_| VaultError::Io)?;
    serde_json::from_slice(&bytes).map_err(|_| VaultError::Serialize)
}

fn write_vault_file(path: &Path, file: &VaultFile) -> Result<(), VaultError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| VaultError::Io)?;
    }
    let bytes = serde_json::to_vec_pretty(file).map_err(|_| VaultError::Serialize)?;
    std::fs::write(path, bytes).map_err(|_| VaultError::Io)
}

fn derive_key(password: &str, salt: &[u8], iterations: u32) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, iterations, &mut key);
    key
}

fn encrypt(key: &[u8; KEY_LEN], nonce: &[u8], plaintext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| VaultError::CryptoUnavailable)?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .encrypt(nonce, plaintext)
        .map_err(|_| VaultError::CryptoUnavailable)
}

fn decrypt(key: &[u8; KEY_LEN], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| VaultError::CryptoUnavailable)?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| VaultError::DecryptFailed)
}

fn decrypt_entry_with_key(
    entry: &EncryptedCredentialEntry,
    key: &[u8; KEY_LEN],
) -> Result<CredentialSecret, VaultError> {
    let decrypted = decrypt(key, &entry.nonce, &entry.encrypted_data)?;
    serde_json::from_slice(&decrypted).map_err(|_| VaultError::DecryptFailed)
}

fn credential_secret_kind(secret: &CredentialSecret) -> CredentialKind {
    match secret {
        CredentialSecret::Password { .. } => CredentialKind::Password,
        CredentialSecret::OAuthToken { .. } => CredentialKind::OAuthToken,
        CredentialSecret::BrowserSession { .. } => CredentialKind::BrowserSession,
        CredentialSecret::ApiKey { .. } => CredentialKind::ApiKey,
        CredentialSecret::SshKey { .. } => CredentialKind::SshKey,
        CredentialSecret::Totp { .. } => CredentialKind::Totp,
    }
}

pub fn build_oauth_refresh_request(
    secret: &CredentialSecret,
) -> Result<OAuthRefreshRequest, VaultError> {
    let CredentialSecret::OAuthToken {
        refresh_token,
        scopes,
        token_url,
        client_id,
        client_secret,
        ..
    } = secret
    else {
        return Err(VaultError::UnsupportedCredentialKind);
    };

    let url = required_oauth_field(token_url.as_deref())?.to_string();
    let client_id = required_oauth_field(client_id.as_deref())?.to_string();
    let refresh_token = required_oauth_field(Some(refresh_token))?.to_string();
    let mut form = vec![
        ("grant_type".to_string(), "refresh_token".to_string()),
        ("refresh_token".to_string(), refresh_token),
        ("client_id".to_string(), client_id),
    ];
    if let Some(client_secret) = optional_oauth_field(client_secret.as_deref()) {
        form.push(("client_secret".to_string(), client_secret.to_string()));
    }
    if !scopes.is_empty() {
        form.push(("scope".to_string(), scopes.join(" ")));
    }

    Ok(OAuthRefreshRequest { url, form })
}

pub fn apply_oauth_refresh_response(
    current: &CredentialSecret,
    raw_response: &str,
    now_ms: u64,
) -> Result<CredentialSecret, VaultError> {
    let CredentialSecret::OAuthToken {
        refresh_token,
        scopes,
        token_url,
        client_id,
        client_secret,
        ..
    } = current
    else {
        return Err(VaultError::UnsupportedCredentialKind);
    };
    let response: OAuthRefreshResponse =
        serde_json::from_str(raw_response).map_err(|_| VaultError::InvalidOAuthResponse)?;
    let access_token = required_oauth_field(Some(&response.access_token))?.to_string();
    let refresh_token = optional_oauth_field(response.refresh_token.as_deref())
        .unwrap_or(refresh_token)
        .to_string();
    let expires_at = now_ms.saturating_add(response.expires_in.saturating_mul(MILLIS_PER_SECOND));

    Ok(CredentialSecret::OAuthToken {
        access_token,
        refresh_token,
        expires_at,
        scopes: scopes.clone(),
        token_url: token_url.clone(),
        client_id: client_id.clone(),
        client_secret: client_secret.clone(),
    })
}

#[derive(Debug, Deserialize)]
struct OAuthRefreshResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

fn required_oauth_field(value: Option<&str>) -> Result<&str, VaultError> {
    optional_oauth_field(value).ok_or(VaultError::MissingOAuthRefreshConfig)
}

fn optional_oauth_field(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn host_from_http_url(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn domain_matches_host(domain: &str, host: &str) -> bool {
    let normalized = host_from_http_url(domain).unwrap_or_else(|| {
        domain
            .trim()
            .trim_start_matches("*.")
            .trim_end_matches('.')
            .to_ascii_lowercase()
    });

    host == normalized || host.ends_with(&format!(".{normalized}"))
}

type HmacSha1 = Hmac<Sha1>;

fn hotp_sha1(secret: &[u8], counter: u64, digits: u32) -> Result<String, VaultError> {
    let mut mac =
        <HmacSha1 as Mac>::new_from_slice(secret).map_err(|_| VaultError::InvalidTotpSecret)?;
    mac.update(&counter.to_be_bytes());
    let hash = mac.finalize().into_bytes();
    let offset = usize::from(hash[hash.len() - 1] & 0x0f);
    let binary = ((u32::from(hash[offset]) & 0x7f) << 24)
        | (u32::from(hash[offset + 1]) << 16)
        | (u32::from(hash[offset + 2]) << 8)
        | u32::from(hash[offset + 3]);
    let modulus = 10u32.pow(digits);
    Ok(format!(
        "{:0width$}",
        binary % modulus,
        width = digits as usize
    ))
}

fn decode_base32_secret(secret: &str) -> Result<Vec<u8>, VaultError> {
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut out = Vec::new();

    for ch in secret.chars() {
        let value = match ch {
            'A'..='Z' => u32::from(ch as u8 - b'A'),
            'a'..='z' => u32::from(ch as u8 - b'a'),
            '2'..='7' => u32::from(ch as u8 - b'2') + 26,
            '=' | ' ' | '-' => continue,
            _ => return Err(VaultError::InvalidTotpSecret),
        };

        buffer = (buffer << 5) | value;
        bits += 5;
        while bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    if out.is_empty() {
        return Err(VaultError::InvalidTotpSecret);
    }
    Ok(out)
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

impl From<io::Error> for VaultError {
    fn from(_: io::Error) -> Self {
        VaultError::Io
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::{AppState, DeviceRegistry};
    use aeon_capture::{CaptureEngine, CaptureStore, EventLog};
    use aeon_store::CIDStore;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::{broadcast, Mutex};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-vault-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn input() -> CredentialEntryInput {
        CredentialEntryInput {
            id: "gmail-work".to_string(),
            kind: CredentialKind::Password,
            label: "Gmail Work".to_string(),
            domains: vec!["mail.google.com".to_string()],
            last_used: 0,
            auto_fill: true,
            secret: CredentialSecret::Password {
                username: "wc@example.test".to_string(),
                password: "super-secret".to_string(),
            },
        }
    }

    fn test_state_with_vault(dir: &std::path::Path, vault: CredentialVaultStore) -> AppState {
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
            credential_vault: Arc::new(Mutex::new(vault)),
            vault_sessions: Arc::new(Mutex::new(CredentialUnlockSessions::default())),
            email_sync: Arc::new(Mutex::new(
                crate::email_sync::EmailSyncStore::new(dir.join("email-sync.json")).unwrap(),
            )),
            query_planner: None,
            verification_codes: Arc::new(Mutex::new(
                crate::bridge::VerificationCodeInbox::default(),
            )),
            devices: Arc::new(Mutex::new(DeviceRegistry::default())),
            connect_urls: Vec::new(),
            relay_url: None,
            relay_space: "test".to_string(),
            device_name: "Test Device".to_string(),
        }
    }

    #[test]
    fn same_password_decrypts_entry_round_trip() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store.add_entry("correct horse", input()).unwrap();

        let secret = store.decrypt_entry("correct horse", "gmail-work").unwrap();

        assert_eq!(
            secret,
            CredentialSecret::Password {
                username: "wc@example.test".to_string(),
                password: "super-secret".to_string(),
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn wrong_password_fails_authentication() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store.add_entry("correct horse", input()).unwrap();

        assert!(matches!(
            store.decrypt_entry("wrong password", "gmail-work"),
            Err(VaultError::DecryptFailed)
        ));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn persisted_vault_does_not_contain_plaintext_secret() {
        let dir = temp_dir();
        let path = dir.join("vault.json");
        let mut store = CredentialVaultStore::new(&path).unwrap();
        store.add_entry("correct horse", input()).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();

        assert!(!raw.contains("super-secret"));
        assert!(!raw.contains("wc@example.test"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn metadata_list_excludes_encrypted_payload_bytes() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store.add_entry("correct horse", input()).unwrap();

        let metadata = store.list_metadata();

        assert_eq!(metadata.len(), 1);
        assert_eq!(metadata[0].id, "gmail-work");
        assert_eq!(metadata[0].label, "Gmail Work");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn totp_code_matches_rfc6238_sha1_vector_with_default_six_digits() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store
            .add_entry(
                "correct horse",
                CredentialEntryInput {
                    id: "totp-rfc".to_string(),
                    kind: CredentialKind::Totp,
                    label: "RFC TOTP".to_string(),
                    domains: Vec::new(),
                    last_used: 0,
                    auto_fill: false,
                    secret: CredentialSecret::Totp {
                        secret: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string(),
                        issuer: "RFC".to_string(),
                    },
                },
            )
            .unwrap();

        let code = store.totp_code("correct horse", "totp-rfc", 59).unwrap();

        assert_eq!(code.code, "287082");
        assert_eq!(code.expires_at, 60);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn totp_handler_returns_code_without_secret_material() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store
            .add_entry(
                "correct horse",
                CredentialEntryInput {
                    id: "totp-rfc".to_string(),
                    kind: CredentialKind::Totp,
                    label: "RFC TOTP".to_string(),
                    domains: Vec::new(),
                    last_used: 0,
                    auto_fill: false,
                    secret: CredentialSecret::Totp {
                        secret: "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".to_string(),
                        issuer: "RFC".to_string(),
                    },
                },
            )
            .unwrap();

        let response = totp_code(
            axum::extract::Path("totp-rfc".to_string()),
            State(test_state_with_vault(&dir, store)),
            Json(TotpCodePayload {
                password: "correct horse".to_string(),
                now: Some(59),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.code, "287082");
        assert_eq!(response.expires_at, 60);
        assert_eq!(response.period, 30);
        let serialized = serde_json::to_string(&response).unwrap();
        assert!(!serialized.contains("GEZDGNBVGY3TQOJQ"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn unlock_handler_rejects_non_loopback_clients() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store.add_entry("correct horse", input()).unwrap();

        let result = unlock_vault(
            ConnectInfo("192.168.1.20:53422".parse().unwrap()),
            State(test_state_with_vault(&dir, store)),
            Json(UnlockVaultPayload {
                password: "correct horse".to_string(),
                ttl_ms: Some(60_000),
            }),
        )
        .await;

        assert_eq!(result.unwrap_err(), StatusCode::FORBIDDEN);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn unlock_session_fills_matching_password_without_listing_other_secrets() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store.add_entry("correct horse", input()).unwrap();
        store
            .add_entry(
                "correct horse",
                CredentialEntryInput {
                    id: "api-key".to_string(),
                    kind: CredentialKind::ApiKey,
                    label: "API".to_string(),
                    domains: vec!["mail.google.com".to_string()],
                    last_used: 0,
                    auto_fill: true,
                    secret: CredentialSecret::ApiKey {
                        key: "do-not-fill".to_string(),
                        header_name: "Authorization".to_string(),
                    },
                },
            )
            .unwrap();
        let state = test_state_with_vault(&dir, store);

        let unlocked = unlock_vault(
            ConnectInfo("127.0.0.1:53422".parse().unwrap()),
            State(state.clone()),
            Json(UnlockVaultPayload {
                password: "correct horse".to_string(),
                ttl_ms: Some(60_000),
            }),
        )
        .await
        .unwrap()
        .0;

        let fill = credential_fill(
            ConnectInfo("127.0.0.1:53423".parse().unwrap()),
            State(state),
            Json(CredentialFillPayload {
                session_id: unlocked.session_id,
                url: "https://mail.google.com/inbox".to_string(),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(
            fill.credential,
            Some(PasswordFillCredential {
                id: "gmail-work".to_string(),
                label: "Gmail Work".to_string(),
                username: "wc@example.test".to_string(),
                password: "super-secret".to_string(),
                expires_at: unlocked.expires_at,
            })
        );
        let serialized = serde_json::to_string(&fill).unwrap();
        assert!(!serialized.contains("do-not-fill"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn password_credential_can_be_read_with_unlocked_key() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store.add_entry("correct horse", input()).unwrap();
        let key = store.unlock_key("correct horse").unwrap();

        let credential = store
            .password_credential_with_key(&key, "gmail-work")
            .unwrap();

        assert_eq!(
            credential,
            PasswordCredential {
                username: "wc@example.test".to_string(),
                password: "super-secret".to_string(),
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oauth_access_token_can_be_read_with_unlocked_key() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store
            .add_entry(
                "correct horse",
                CredentialEntryInput {
                    id: "gmail-oauth".to_string(),
                    kind: CredentialKind::OAuthToken,
                    label: "Gmail OAuth".to_string(),
                    domains: vec!["gmail.googleapis.com".to_string()],
                    last_used: 0,
                    auto_fill: false,
                    secret: CredentialSecret::OAuthToken {
                        access_token: "access-token".to_string(),
                        refresh_token: "refresh-token".to_string(),
                        expires_at: 1_771_000_000_000,
                        scopes: vec!["https://www.googleapis.com/auth/gmail.readonly".to_string()],
                        token_url: None,
                        client_id: None,
                        client_secret: None,
                    },
                },
            )
            .unwrap();
        let key = store.unlock_key("correct horse").unwrap();

        let token = store
            .oauth_access_token_with_key(&key, "gmail-oauth")
            .unwrap();

        assert_eq!(token, "access-token");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oauth_refresh_request_uses_refresh_token_grant() {
        let secret = CredentialSecret::OAuthToken {
            access_token: "old-access".to_string(),
            refresh_token: "refresh-token".to_string(),
            expires_at: 1_000,
            scopes: vec!["Mail.Read".to_string()],
            token_url: Some("https://login.example.test/oauth2/v2.0/token".to_string()),
            client_id: Some("client-id".to_string()),
            client_secret: Some("client-secret".to_string()),
        };

        let request = build_oauth_refresh_request(&secret).unwrap();

        assert_eq!(request.url, "https://login.example.test/oauth2/v2.0/token");
        assert_eq!(
            request.form,
            vec![
                ("grant_type".to_string(), "refresh_token".to_string()),
                ("refresh_token".to_string(), "refresh-token".to_string()),
                ("client_id".to_string(), "client-id".to_string()),
                ("client_secret".to_string(), "client-secret".to_string()),
                ("scope".to_string(), "Mail.Read".to_string()),
            ]
        );
    }

    #[test]
    fn oauth_refresh_response_updates_access_and_refresh_tokens() {
        let current = CredentialSecret::OAuthToken {
            access_token: "old-access".to_string(),
            refresh_token: "old-refresh".to_string(),
            expires_at: 1_000,
            scopes: vec!["Mail.Read".to_string()],
            token_url: Some("https://login.example.test/oauth2/v2.0/token".to_string()),
            client_id: Some("client-id".to_string()),
            client_secret: Some("client-secret".to_string()),
        };

        let refreshed = apply_oauth_refresh_response(
            &current,
            r#"{
                "access_token": "new-access",
                "refresh_token": "new-refresh",
                "expires_in": 3600
            }"#,
            10_000,
        )
        .unwrap();

        let CredentialSecret::OAuthToken {
            access_token,
            refresh_token,
            expires_at,
            scopes,
            token_url,
            client_id,
            client_secret,
        } = refreshed
        else {
            panic!("expected OAuth token");
        };

        assert_eq!(access_token, "new-access");
        assert_eq!(refresh_token, "new-refresh");
        assert_eq!(expires_at, 3_610_000);
        assert_eq!(scopes, vec!["Mail.Read".to_string()]);
        assert_eq!(
            token_url.as_deref(),
            Some("https://login.example.test/oauth2/v2.0/token")
        );
        assert_eq!(client_id.as_deref(), Some("client-id"));
        assert_eq!(client_secret.as_deref(), Some("client-secret"));
    }

    #[test]
    fn refreshed_oauth_token_is_persisted_in_vault() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store
            .add_entry(
                "correct horse",
                CredentialEntryInput {
                    id: "outlook-oauth".to_string(),
                    kind: CredentialKind::OAuthToken,
                    label: "Outlook OAuth".to_string(),
                    domains: vec!["graph.microsoft.com".to_string()],
                    last_used: 0,
                    auto_fill: false,
                    secret: CredentialSecret::OAuthToken {
                        access_token: "old-access".to_string(),
                        refresh_token: "old-refresh".to_string(),
                        expires_at: 1_000,
                        scopes: vec!["Mail.Read".to_string()],
                        token_url: Some("https://login.example.test/oauth2/v2.0/token".to_string()),
                        client_id: Some("client-id".to_string()),
                        client_secret: Some("client-secret".to_string()),
                    },
                },
            )
            .unwrap();
        let key = store.unlock_key("correct horse").unwrap();
        let refreshed = apply_oauth_refresh_response(
            &store
                .decrypt_entry("correct horse", "outlook-oauth")
                .unwrap(),
            r#"{"access_token":"new-access","expires_in":60}"#,
            10_000,
        )
        .unwrap();

        store
            .replace_secret_with_key(&key, "outlook-oauth", refreshed)
            .unwrap();
        let reloaded = CredentialVaultStore::new(dir.join("vault.json")).unwrap();

        assert_eq!(
            reloaded
                .oauth_access_token_with_key(&key, "outlook-oauth")
                .unwrap(),
            "new-access"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oauth_token_state_uses_valid_access_token_before_refresh_window() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store
            .add_entry(
                "correct horse",
                CredentialEntryInput {
                    id: "gmail-oauth".to_string(),
                    kind: CredentialKind::OAuthToken,
                    label: "Gmail OAuth".to_string(),
                    domains: vec!["gmail.googleapis.com".to_string()],
                    last_used: 0,
                    auto_fill: false,
                    secret: CredentialSecret::OAuthToken {
                        access_token: "usable-access".to_string(),
                        refresh_token: "refresh-token".to_string(),
                        expires_at: 300_000,
                        scopes: vec!["gmail.readonly".to_string()],
                        token_url: Some("https://oauth.example.test/token".to_string()),
                        client_id: Some("client-id".to_string()),
                        client_secret: None,
                    },
                },
            )
            .unwrap();
        let key = store.unlock_key("correct horse").unwrap();

        let state = store
            .oauth_access_token_state_with_key(&key, "gmail-oauth", 100_000)
            .unwrap();

        assert_eq!(
            state,
            OAuthAccessTokenState::Ready("usable-access".to_string())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn oauth_token_state_builds_refresh_request_when_expiring() {
        let dir = temp_dir();
        let mut store = CredentialVaultStore::new(dir.join("vault.json")).unwrap();
        store
            .add_entry(
                "correct horse",
                CredentialEntryInput {
                    id: "outlook-oauth".to_string(),
                    kind: CredentialKind::OAuthToken,
                    label: "Outlook OAuth".to_string(),
                    domains: vec!["graph.microsoft.com".to_string()],
                    last_used: 0,
                    auto_fill: false,
                    secret: CredentialSecret::OAuthToken {
                        access_token: "old-access".to_string(),
                        refresh_token: "refresh-token".to_string(),
                        expires_at: 120_000,
                        scopes: vec!["Mail.Read".to_string()],
                        token_url: Some("https://oauth.example.test/token".to_string()),
                        client_id: Some("client-id".to_string()),
                        client_secret: None,
                    },
                },
            )
            .unwrap();
        let key = store.unlock_key("correct horse").unwrap();

        let state = store
            .oauth_access_token_state_with_key(&key, "outlook-oauth", 61_000)
            .unwrap();

        let OAuthAccessTokenState::NeedsRefresh { request, current } = state else {
            panic!("expected refresh request");
        };
        assert_eq!(request.url, "https://oauth.example.test/token");
        assert_eq!(
            request.form,
            vec![
                ("grant_type".to_string(), "refresh_token".to_string()),
                ("refresh_token".to_string(), "refresh-token".to_string()),
                ("client_id".to_string(), "client-id".to_string()),
                ("scope".to_string(), "Mail.Read".to_string()),
            ]
        );
        assert_eq!(
            current,
            CredentialSecret::OAuthToken {
                access_token: "old-access".to_string(),
                refresh_token: "refresh-token".to_string(),
                expires_at: 120_000,
                scopes: vec!["Mail.Read".to_string()],
                token_url: Some("https://oauth.example.test/token".to_string()),
                client_id: Some("client-id".to_string()),
                client_secret: None,
            }
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
