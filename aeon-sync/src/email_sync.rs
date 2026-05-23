use crate::email_imap::{fetch_imap_messages, ImapCredentials, ImapFetchError, ImapMailboxConfig};
use crate::server::AppState;
use crate::vault::{
    apply_oauth_refresh_response, CredentialSecret, OAuthAccessTokenState, OAuthRefreshRequest,
};
use aeon_capture::bridge::EmailBridgePayload;
use aeon_capture::{hex_cid, CaptureEntry};
use axum::extract::{ConnectInfo, Path as AxumPath, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

const MAX_SEEN_MESSAGE_IDS: usize = 2048;
const MAX_PROVIDER_SYNC_LIMIT: usize = 50;
const GMAIL_API_BASE: &str = "https://gmail.googleapis.com/gmail/v1";
const OUTLOOK_API_BASE: &str = "https://graph.microsoft.com/v1.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EmailAccountConfig {
    pub id: String,
    pub label: String,
    pub address: String,
    pub provider: EmailProviderConfig,
    pub labels: Vec<String>,
    pub credential_ref: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EmailProviderConfig {
    Imap {
        host: String,
        port: u16,
        tls: bool,
        mailbox: String,
    },
    GmailApi {
        account_id: String,
    },
    OutlookApi {
        account_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FetchedEmailMessage {
    pub uid: u64,
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub subject: String,
    pub body_preview: String,
    pub received_at: u64,
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct EmailCursor {
    pub last_uid: Option<u64>,
    pub seen_message_ids: Vec<String>,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedEmailCapture {
    pub uid: u64,
    pub message_id: String,
    pub entry: CaptureEntry,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmailImportResult {
    pub imported: Vec<ImportedEmailCapture>,
    pub skipped: usize,
    pub cursor: EmailCursor,
}

#[derive(Debug)]
pub struct EmailSyncStore {
    path: PathBuf,
    state: EmailSyncState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EmailSyncState {
    accounts: Vec<EmailAccountConfig>,
    cursors: HashMap<String, EmailCursor>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EmailSyncError {
    Io,
    Serialize,
    MissingAccount,
}

#[derive(Debug, Deserialize)]
pub struct ImportEmailMessagesPayload {
    pub messages: Vec<FetchedEmailMessage>,
}

#[derive(Debug, Deserialize)]
pub struct SyncEmailAccountPayload {
    pub session_id: String,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ImportedEmailPayload {
    pub uid: u64,
    pub message_id: String,
    pub cid: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct ImportEmailMessagesResponse {
    pub imported: Vec<ImportedEmailPayload>,
    pub skipped: usize,
    pub cursor: EmailCursor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderListRequest {
    pub url: String,
    pub authorization: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EmailProviderError {
    UnsupportedProvider,
    MissingField,
    InvalidJson,
    InvalidTimestamp,
    Http,
}

impl EmailSyncStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self, EmailSyncError> {
        let path = path.as_ref().to_path_buf();
        let state = if path.exists() {
            let bytes = std::fs::read(&path).map_err(|_| EmailSyncError::Io)?;
            serde_json::from_slice(&bytes).map_err(|_| EmailSyncError::Serialize)?
        } else {
            EmailSyncState::default()
        };
        Ok(Self { path, state })
    }

    pub fn list_accounts(&self) -> Vec<EmailAccountConfig> {
        self.state.accounts.clone()
    }

    pub fn account(&self, account_id: &str) -> Option<EmailAccountConfig> {
        self.state
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .cloned()
    }

    pub fn upsert_account(&mut self, account: EmailAccountConfig) -> Result<(), EmailSyncError> {
        if let Some(existing) = self
            .state
            .accounts
            .iter_mut()
            .find(|existing| existing.id == account.id)
        {
            *existing = account;
        } else {
            self.state.accounts.push(account);
        }
        self.state.accounts.sort_by(|a, b| a.id.cmp(&b.id));
        self.save()
    }

    pub fn prepare_import(
        &mut self,
        account_id: &str,
        messages: Vec<FetchedEmailMessage>,
        identity_id: [u8; 32],
        device_id: [u8; 16],
        now_ms: u64,
    ) -> Result<EmailImportResult, EmailSyncError> {
        let account = self
            .state
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .cloned()
            .ok_or(EmailSyncError::MissingAccount)?;
        let cursor = self
            .state
            .cursors
            .entry(account_id.to_string())
            .or_default();
        let mut seen = cursor
            .seen_message_ids
            .iter()
            .cloned()
            .collect::<HashSet<_>>();
        let mut imported = Vec::new();
        let mut skipped = 0usize;
        let mut max_uid = cursor.last_uid;

        for message in messages {
            if !seen.insert(message.message_id.clone()) {
                skipped += 1;
                continue;
            }
            max_uid = Some(max_uid.map_or(message.uid, |uid| uid.max(message.uid)));
            let mut entry = email_entry(&account, &message);
            entry.by = identity_id;
            entry.device = device_id;
            imported.push(ImportedEmailCapture {
                uid: message.uid,
                message_id: message.message_id,
                entry,
            });
        }

        cursor.last_uid = max_uid;
        cursor.updated_at = now_ms;
        cursor.seen_message_ids = bounded_seen_message_ids(seen);
        let cursor_snapshot = cursor.clone();
        self.save()?;

        Ok(EmailImportResult {
            imported,
            skipped,
            cursor: cursor_snapshot,
        })
    }

    fn save(&self) -> Result<(), EmailSyncError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| EmailSyncError::Io)?;
        }
        let bytes =
            serde_json::to_vec_pretty(&self.state).map_err(|_| EmailSyncError::Serialize)?;
        std::fs::write(&self.path, bytes).map_err(|_| EmailSyncError::Io)
    }
}

pub async fn list_email_accounts(State(state): State<AppState>) -> Json<Vec<EmailAccountConfig>> {
    Json(state.email_sync.lock().await.list_accounts())
}

pub async fn upsert_email_account(
    State(state): State<AppState>,
    Json(payload): Json<EmailAccountConfig>,
) -> Result<Json<Vec<EmailAccountConfig>>, StatusCode> {
    let mut store = state.email_sync.lock().await;
    store.upsert_account(payload).map_err(email_sync_status)?;
    Ok(Json(store.list_accounts()))
}

pub async fn import_email_messages(
    AxumPath(account_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(payload): Json<ImportEmailMessagesPayload>,
) -> Result<Json<ImportEmailMessagesResponse>, StatusCode> {
    let result = {
        let mut store = state.email_sync.lock().await;
        store
            .prepare_import(
                &account_id,
                payload.messages,
                state.identity_id,
                state.device_id,
                now_ms(),
            )
            .map_err(email_sync_status)?
    };

    let mut imported = Vec::new();
    for planned in result.imported {
        let cid = state
            .capture_engine
            .capture(planned.entry)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        imported.push(ImportedEmailPayload {
            uid: planned.uid,
            message_id: planned.message_id,
            cid: hex_cid(&cid),
        });
    }

    Ok(Json(ImportEmailMessagesResponse {
        imported,
        skipped: result.skipped,
        cursor: result.cursor,
    }))
}

pub async fn sync_email_account(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    AxumPath(account_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(payload): Json<SyncEmailAccountPayload>,
) -> Result<Json<ImportEmailMessagesResponse>, StatusCode> {
    require_loopback(addr)?;
    let (key, _) = state
        .vault_sessions
        .lock()
        .await
        .session_key(&payload.session_id, now_ms())
        .ok_or(StatusCode::UNAUTHORIZED)?;
    let account = state
        .email_sync
        .lock()
        .await
        .account(&account_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let credential_ref = account
        .credential_ref
        .as_deref()
        .ok_or(StatusCode::BAD_REQUEST)?;
    let limit = payload.limit.unwrap_or(MAX_PROVIDER_SYNC_LIMIT);
    let messages = match &account.provider {
        EmailProviderConfig::Imap {
            host,
            port,
            tls,
            mailbox,
        } => {
            let credentials =
                resolve_imap_credentials_for_sync(&state, key, credential_ref).await?;
            let mailbox = ImapMailboxConfig {
                host: host.clone(),
                port: *port,
                tls: *tls,
                mailbox: mailbox.clone(),
            };
            fetch_imap_messages(&mailbox, &credentials, limit)
                .await
                .map_err(imap_status)?
        }
        EmailProviderConfig::GmailApi { .. } | EmailProviderConfig::OutlookApi { .. } => {
            let access_token =
                resolve_oauth_access_token_for_sync(&state, key, credential_ref, now_ms()).await?;
            fetch_provider_messages(&account, &access_token, limit)
                .await
                .map_err(provider_status)?
        }
    };

    import_fetched_messages(account_id, messages, state).await
}

async fn resolve_imap_credentials_for_sync(
    state: &AppState,
    key: [u8; 32],
    credential_ref: &str,
) -> Result<ImapCredentials, StatusCode> {
    let credential = state
        .credential_vault
        .lock()
        .await
        .password_credential_with_key(&key, credential_ref)
        .map_err(vault_oauth_status)?;
    Ok(ImapCredentials {
        username: credential.username,
        password: credential.password,
    })
}

async fn resolve_oauth_access_token_for_sync(
    state: &AppState,
    key: [u8; 32],
    credential_ref: &str,
    now_ms: u64,
) -> Result<String, StatusCode> {
    let token_state = {
        state
            .credential_vault
            .lock()
            .await
            .oauth_access_token_state_with_key(&key, credential_ref, now_ms)
            .map_err(vault_oauth_status)?
    };

    match token_state {
        OAuthAccessTokenState::Ready(access_token) => Ok(access_token),
        OAuthAccessTokenState::NeedsRefresh { request, current } => {
            let response = send_oauth_refresh_request(&request)
                .await
                .map_err(provider_status)?;
            let refreshed = apply_oauth_refresh_response(&current, &response, now_ms)
                .map_err(vault_oauth_status)?;
            let CredentialSecret::OAuthToken { access_token, .. } = &refreshed else {
                return Err(StatusCode::BAD_REQUEST);
            };
            let access_token = access_token.clone();
            state
                .credential_vault
                .lock()
                .await
                .replace_secret_with_key(&key, credential_ref, refreshed)
                .map_err(vault_oauth_status)?;
            Ok(access_token)
        }
    }
}

fn email_sync_status(error: EmailSyncError) -> StatusCode {
    match error {
        EmailSyncError::MissingAccount => StatusCode::NOT_FOUND,
        EmailSyncError::Io | EmailSyncError::Serialize => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn import_fetched_messages(
    account_id: String,
    messages: Vec<FetchedEmailMessage>,
    state: AppState,
) -> Result<Json<ImportEmailMessagesResponse>, StatusCode> {
    let result = {
        let mut store = state.email_sync.lock().await;
        store
            .prepare_import(
                &account_id,
                messages,
                state.identity_id,
                state.device_id,
                now_ms(),
            )
            .map_err(email_sync_status)?
    };

    let mut imported = Vec::new();
    for planned in result.imported {
        let cid = state
            .capture_engine
            .capture(planned.entry)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        imported.push(ImportedEmailPayload {
            uid: planned.uid,
            message_id: planned.message_id,
            cid: hex_cid(&cid),
        });
    }

    Ok(Json(ImportEmailMessagesResponse {
        imported,
        skipped: result.skipped,
        cursor: result.cursor,
    }))
}

fn require_loopback(addr: SocketAddr) -> Result<(), StatusCode> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

fn vault_oauth_status(error: crate::vault::VaultError) -> StatusCode {
    match error {
        crate::vault::VaultError::MissingEntry => StatusCode::NOT_FOUND,
        crate::vault::VaultError::DecryptFailed => StatusCode::UNAUTHORIZED,
        crate::vault::VaultError::UnsupportedCredentialKind
        | crate::vault::VaultError::MissingOAuthRefreshConfig => StatusCode::BAD_REQUEST,
        crate::vault::VaultError::InvalidOAuthResponse => StatusCode::BAD_GATEWAY,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

fn provider_status(error: EmailProviderError) -> StatusCode {
    match error {
        EmailProviderError::UnsupportedProvider
        | EmailProviderError::MissingField
        | EmailProviderError::InvalidJson
        | EmailProviderError::InvalidTimestamp => StatusCode::BAD_GATEWAY,
        EmailProviderError::Http => StatusCode::BAD_GATEWAY,
    }
}

fn imap_status(error: ImapFetchError) -> StatusCode {
    match error {
        ImapFetchError::Network | ImapFetchError::Protocol => StatusCode::BAD_GATEWAY,
        ImapFetchError::TlsConfig => StatusCode::BAD_REQUEST,
    }
}

async fn send_oauth_refresh_request(
    request: &OAuthRefreshRequest,
) -> Result<String, EmailProviderError> {
    reqwest::Client::new()
        .post(&request.url)
        .form(&request.form)
        .send()
        .await
        .map_err(|_| EmailProviderError::Http)?
        .error_for_status()
        .map_err(|_| EmailProviderError::Http)?
        .text()
        .await
        .map_err(|_| EmailProviderError::Http)
}

fn email_entry(account: &EmailAccountConfig, message: &FetchedEmailMessage) -> CaptureEntry {
    let mut entry = EmailBridgePayload {
        message_id: message.message_id.clone(),
        from: message.from.clone(),
        to: message.to.clone(),
        subject: message.subject.clone(),
        body_preview: message.body_preview.clone(),
        received_at: message.received_at,
        labels: if message.labels.is_empty() {
            account.labels.clone()
        } else {
            message.labels.clone()
        },
    }
    .into_capture_entry();
    entry
        .meta
        .extra
        .insert("account_id".to_string(), account.id.clone());
    entry
        .meta
        .extra
        .insert("account_label".to_string(), account.label.clone());
    entry
        .meta
        .extra
        .insert("uid".to_string(), message.uid.to_string());
    entry
        .meta
        .extra
        .insert("provider".to_string(), account.provider.key().to_string());
    if let Some(credential_ref) = &account.credential_ref {
        entry
            .meta
            .extra
            .insert("credential_ref".to_string(), credential_ref.clone());
    }
    entry
}

impl EmailProviderConfig {
    fn key(&self) -> &'static str {
        match self {
            EmailProviderConfig::Imap { .. } => "imap",
            EmailProviderConfig::GmailApi { .. } => "gmail",
            EmailProviderConfig::OutlookApi { .. } => "outlook",
        }
    }
}

pub fn build_provider_list_request(
    account: &EmailAccountConfig,
    access_token: &str,
    limit: usize,
) -> Result<ProviderListRequest, EmailProviderError> {
    let limit = limit.clamp(1, MAX_PROVIDER_SYNC_LIMIT);
    let authorization = Some(format!("Bearer {access_token}"));
    let url = match &account.provider {
        EmailProviderConfig::GmailApi { account_id } => {
            let mut url = format!(
                "{GMAIL_API_BASE}/users/{}/messages?maxResults={limit}",
                percent_encode(account_id)
            );
            for label in &account.labels {
                if !label.trim().is_empty() {
                    url.push_str("&labelIds=");
                    url.push_str(&percent_encode(label.trim()));
                }
            }
            url
        }
        EmailProviderConfig::OutlookApi { account_id } => {
            let owner = if account_id == "me" {
                "me".to_string()
            } else {
                format!("users/{}", percent_encode(account_id))
            };
            format!(
                "{OUTLOOK_API_BASE}/{owner}/messages?$top={limit}&$select=id,subject,sender,toRecipients,receivedDateTime,bodyPreview"
            )
        }
        EmailProviderConfig::Imap { .. } => return Err(EmailProviderError::UnsupportedProvider),
    };

    Ok(ProviderListRequest { url, authorization })
}

pub fn build_gmail_message_request(
    account_id: &str,
    message_id: &str,
    access_token: &str,
) -> ProviderListRequest {
    ProviderListRequest {
        url: format!(
            "{GMAIL_API_BASE}/users/{}/messages/{}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=To",
            percent_encode(account_id),
            percent_encode(message_id)
        ),
        authorization: Some(format!("Bearer {access_token}")),
    }
}

pub async fn fetch_provider_messages(
    account: &EmailAccountConfig,
    access_token: &str,
    limit: usize,
) -> Result<Vec<FetchedEmailMessage>, EmailProviderError> {
    let list_request = build_provider_list_request(account, access_token, limit)?;
    let list_response = provider_get(&list_request).await?;
    match &account.provider {
        EmailProviderConfig::GmailApi { account_id } => {
            let message_ids = parse_gmail_message_ids(&list_response)?;
            let mut messages = Vec::new();
            for message_id in message_ids
                .into_iter()
                .take(limit.clamp(1, MAX_PROVIDER_SYNC_LIMIT))
            {
                let request = build_gmail_message_request(account_id, &message_id, access_token);
                let response = provider_get(&request).await?;
                messages.push(parse_gmail_message_json(&response)?);
            }
            Ok(messages)
        }
        EmailProviderConfig::OutlookApi { .. } => parse_outlook_messages_json(&list_response),
        EmailProviderConfig::Imap { .. } => Err(EmailProviderError::UnsupportedProvider),
    }
}

async fn provider_get(request: &ProviderListRequest) -> Result<String, EmailProviderError> {
    let client = reqwest::Client::new();
    let mut builder = client.get(&request.url);
    if let Some(authorization) = &request.authorization {
        builder = builder.header(reqwest::header::AUTHORIZATION, authorization);
    }
    builder
        .send()
        .await
        .map_err(|_| EmailProviderError::Http)?
        .error_for_status()
        .map_err(|_| EmailProviderError::Http)?
        .text()
        .await
        .map_err(|_| EmailProviderError::Http)
}

pub fn parse_gmail_message_ids(raw: &str) -> Result<Vec<String>, EmailProviderError> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| EmailProviderError::InvalidJson)?;
    let messages = value
        .get("messages")
        .and_then(serde_json::Value::as_array)
        .ok_or(EmailProviderError::MissingField)?;
    Ok(messages
        .iter()
        .filter_map(|message| message.get("id"))
        .filter_map(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
        .collect())
}

pub fn parse_gmail_message_json(raw: &str) -> Result<FetchedEmailMessage, EmailProviderError> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| EmailProviderError::InvalidJson)?;
    gmail_message_from_value(&value)
}

pub fn parse_outlook_messages_json(
    raw: &str,
) -> Result<Vec<FetchedEmailMessage>, EmailProviderError> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|_| EmailProviderError::InvalidJson)?;
    let messages = value
        .get("value")
        .and_then(serde_json::Value::as_array)
        .ok_or(EmailProviderError::MissingField)?;
    messages.iter().map(outlook_message_from_value).collect()
}

fn gmail_message_from_value(
    value: &serde_json::Value,
) -> Result<FetchedEmailMessage, EmailProviderError> {
    let id = required_str(value, "id")?;
    let headers = value
        .pointer("/payload/headers")
        .and_then(serde_json::Value::as_array)
        .ok_or(EmailProviderError::MissingField)?;
    let subject = header_value(headers, "Subject").unwrap_or_else(|| "(no subject)".to_string());
    let from = header_value(headers, "From").unwrap_or_default();
    let to = header_value(headers, "To")
        .map(|value| split_recipients(&value))
        .unwrap_or_default();
    let received_at = value
        .get("internalDate")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or(EmailProviderError::InvalidTimestamp)?;
    let labels = value
        .get("labelIds")
        .and_then(serde_json::Value::as_array)
        .map(|labels| {
            labels
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let body_preview = value
        .get("snippet")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let message_id = format!("gmail:{id}");

    Ok(FetchedEmailMessage {
        uid: stable_uid(&message_id),
        message_id,
        from,
        to,
        subject,
        body_preview,
        received_at,
        labels,
    })
}

fn outlook_message_from_value(
    value: &serde_json::Value,
) -> Result<FetchedEmailMessage, EmailProviderError> {
    let id = required_str(value, "id")?;
    let message_id = format!("outlook:{id}");
    let subject = value
        .get("subject")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("(no subject)")
        .to_string();
    let from = value
        .pointer("/sender/emailAddress/address")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();
    let to = value
        .get("toRecipients")
        .and_then(serde_json::Value::as_array)
        .map(|recipients| {
            recipients
                .iter()
                .filter_map(|recipient| {
                    recipient
                        .pointer("/emailAddress/address")
                        .and_then(serde_json::Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let received_at = value
        .get("receivedDateTime")
        .and_then(serde_json::Value::as_str)
        .ok_or(EmailProviderError::MissingField)
        .and_then(parse_rfc3339_utc_ms)?;
    let body_preview = value
        .get("bodyPreview")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();

    Ok(FetchedEmailMessage {
        uid: stable_uid(&message_id),
        message_id,
        from,
        to,
        subject,
        body_preview,
        received_at,
        labels: Vec::new(),
    })
}

fn required_str<'a>(
    value: &'a serde_json::Value,
    key: &str,
) -> Result<&'a str, EmailProviderError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or(EmailProviderError::MissingField)
}

fn header_value(headers: &[serde_json::Value], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|header| {
            header
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        })
        .and_then(|header| header.get("value"))
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

fn split_recipients(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|recipient| !recipient.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn stable_uid(value: &str) -> u64 {
    let hash = blake3::hash(value.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&hash.as_bytes()[..8]);
    u64::from_be_bytes(bytes)
}

fn parse_rfc3339_utc_ms(value: &str) -> Result<u64, EmailProviderError> {
    let value = value
        .strip_suffix('Z')
        .ok_or(EmailProviderError::InvalidTimestamp)?;
    let (date, time) = value
        .split_once('T')
        .ok_or(EmailProviderError::InvalidTimestamp)?;
    let mut date_parts = date.split('-');
    let year = parse_i32(date_parts.next())?;
    let month = parse_u32(date_parts.next())?;
    let day = parse_u32(date_parts.next())?;
    if date_parts.next().is_some() {
        return Err(EmailProviderError::InvalidTimestamp);
    }
    let time = time.split('.').next().unwrap_or(time);
    let mut time_parts = time.split(':');
    let hour = parse_u32(time_parts.next())?;
    let minute = parse_u32(time_parts.next())?;
    let second = parse_u32(time_parts.next())?;
    if time_parts.next().is_some()
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return Err(EmailProviderError::InvalidTimestamp);
    }
    let days = days_from_civil(year, month, day);
    if days < 0 {
        return Err(EmailProviderError::InvalidTimestamp);
    }
    Ok((days as u64 * 86_400 + hour as u64 * 3_600 + minute as u64 * 60 + second as u64) * 1_000)
}

fn parse_i32(value: Option<&str>) -> Result<i32, EmailProviderError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or(EmailProviderError::InvalidTimestamp)
}

fn parse_u32(value: Option<&str>) -> Result<u32, EmailProviderError> {
    value
        .and_then(|value| value.parse().ok())
        .ok_or(EmailProviderError::InvalidTimestamp)
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let day = day as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) as i64
}

fn percent_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}

fn bounded_seen_message_ids(seen: HashSet<String>) -> Vec<String> {
    let mut values = seen.into_iter().collect::<Vec<_>>();
    values.sort();
    if values.len() > MAX_SEEN_MESSAGE_IDS {
        values.drain(0..values.len() - MAX_SEEN_MESSAGE_IDS);
    }
    values
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
    use aeon_capture::bridge::BRIDGE_KIND_KEY;
    use aeon_capture::{CaptureEngine, CaptureKind, CaptureSource, CaptureStore, EventLog};
    use aeon_store::CIDStore;
    use axum::extract::ConnectInfo;
    use axum::Json;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tokio::sync::{broadcast, Mutex};

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-email-sync-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn account() -> EmailAccountConfig {
        EmailAccountConfig {
            id: "work-mail".to_string(),
            label: "Work Mail".to_string(),
            address: "wc@example.test".to_string(),
            provider: EmailProviderConfig::Imap {
                host: "imap.example.test".to_string(),
                port: 993,
                tls: true,
                mailbox: "INBOX".to_string(),
            },
            labels: vec!["inbox".to_string()],
            credential_ref: Some("vault-work-mail".to_string()),
            enabled: true,
        }
    }

    fn message(uid: u64, message_id: &str, subject: &str) -> FetchedEmailMessage {
        FetchedEmailMessage {
            uid,
            message_id: message_id.to_string(),
            from: "sender@example.test".to_string(),
            to: vec!["wc@example.test".to_string()],
            subject: subject.to_string(),
            body_preview: format!("preview for {subject}"),
            received_at: 1_771_000_000_000 + uid,
            labels: vec!["inbox".to_string()],
        }
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
            verification_codes: Arc::new(Mutex::new(
                crate::bridge::VerificationCodeInbox::default(),
            )),
            devices: Arc::new(Mutex::new(crate::server::DeviceRegistry::default())),
            connect_urls: Vec::new(),
            relay_url: None,
            relay_space: "test".to_string(),
            device_name: "Test Device".to_string(),
        }
    }

    #[test]
    fn store_round_trips_email_account_config() {
        let dir = temp_dir();
        let path = dir.join("email-sync.json");
        let mut store = EmailSyncStore::new(&path).unwrap();

        store.upsert_account(account()).unwrap();
        let reloaded = EmailSyncStore::new(&path).unwrap();

        assert_eq!(reloaded.list_accounts(), vec![account()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn import_batch_deduplicates_messages_and_updates_cursor() {
        let dir = temp_dir();
        let mut store = EmailSyncStore::new(dir.join("email-sync.json")).unwrap();
        store.upsert_account(account()).unwrap();

        let result = store
            .prepare_import(
                "work-mail",
                vec![
                    message(1, "m-1", "First"),
                    message(1, "m-1", "First duplicate"),
                    message(2, "m-2", "Second"),
                ],
                [1u8; 32],
                [2u8; 16],
                1_771_000_001_000,
            )
            .unwrap();

        assert_eq!(result.imported.len(), 2);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.cursor.last_uid, Some(2));

        let first = &result.imported[0].entry;
        assert_eq!(first.kind, CaptureKind::Text);
        assert_eq!(
            first.source,
            CaptureSource::AppApi {
                app: "bridge.email".to_string()
            }
        );
        assert_eq!(first.by, [1u8; 32]);
        assert_eq!(first.device, [2u8; 16]);
        assert_eq!(first.meta.title.as_deref(), Some("First"));
        assert_eq!(
            first.meta.extra.get(BRIDGE_KIND_KEY).map(String::as_str),
            Some("email")
        );
        assert_eq!(
            first.meta.extra.get("account_id").map(String::as_str),
            Some("work-mail")
        );
        assert_eq!(first.meta.extra.get("uid").map(String::as_str), Some("1"));

        let replay = store
            .prepare_import(
                "work-mail",
                vec![message(1, "m-1", "First"), message(2, "m-2", "Second")],
                [1u8; 32],
                [2u8; 16],
                1_771_000_002_000,
            )
            .unwrap();

        assert!(replay.imported.is_empty());
        assert_eq!(replay.skipped, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn gmail_list_request_uses_bearer_token_and_label_filters() {
        let mut account = account();
        account.provider = EmailProviderConfig::GmailApi {
            account_id: "me".to_string(),
        };
        account.labels = vec!["INBOX".to_string(), "IMPORTANT".to_string()];

        let request = build_provider_list_request(&account, "access-token", 25).unwrap();

        assert_eq!(
            request.url,
            "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults=25&labelIds=INBOX&labelIds=IMPORTANT"
        );
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
    }

    #[test]
    fn gmail_message_get_request_asks_for_metadata_headers() {
        let request = build_gmail_message_request("me", "gmail-id-1", "access-token");

        assert_eq!(
            request.url,
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/gmail-id-1?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=To"
        );
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
    }

    #[test]
    fn outlook_list_request_uses_graph_select_and_top() {
        let mut account = account();
        account.provider = EmailProviderConfig::OutlookApi {
            account_id: "me".to_string(),
        };

        let request = build_provider_list_request(&account, "access-token", 10).unwrap();

        assert_eq!(
            request.url,
            "https://graph.microsoft.com/v1.0/me/messages?$top=10&$select=id,subject,sender,toRecipients,receivedDateTime,bodyPreview"
        );
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
    }

    #[test]
    fn gmail_message_response_converts_to_fetched_email() {
        let message = parse_gmail_message_json(
            r#"{
                "id": "gmail-id-1",
                "labelIds": ["INBOX"],
                "snippet": "AEON build completed",
                "internalDate": "1771000000123",
                "payload": {
                    "headers": [
                        { "name": "Subject", "value": "Build finished" },
                        { "name": "From", "value": "Sender <sender@example.test>" },
                        { "name": "To", "value": "wc@example.test, ops@example.test" }
                    ]
                }
            }"#,
        )
        .unwrap();

        assert_eq!(message.message_id, "gmail:gmail-id-1");
        assert_eq!(message.subject, "Build finished");
        assert_eq!(message.from, "Sender <sender@example.test>");
        assert_eq!(
            message.to,
            vec![
                "wc@example.test".to_string(),
                "ops@example.test".to_string()
            ]
        );
        assert_eq!(message.body_preview, "AEON build completed");
        assert_eq!(message.received_at, 1_771_000_000_123);
        assert_eq!(message.labels, vec!["INBOX".to_string()]);
    }

    #[test]
    fn outlook_messages_response_converts_to_fetched_emails() {
        let messages = parse_outlook_messages_json(
            r#"{
                "value": [{
                    "id": "outlook-id-1",
                    "subject": "Build finished",
                    "sender": { "emailAddress": { "address": "sender@example.test" } },
                    "toRecipients": [
                        { "emailAddress": { "address": "wc@example.test" } }
                    ],
                    "receivedDateTime": "1970-01-01T00:00:01Z",
                    "bodyPreview": "AEON build completed"
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message_id, "outlook:outlook-id-1");
        assert_eq!(messages[0].subject, "Build finished");
        assert_eq!(messages[0].from, "sender@example.test");
        assert_eq!(messages[0].to, vec!["wc@example.test".to_string()]);
        assert_eq!(messages[0].received_at, 1_000);
        assert_eq!(messages[0].body_preview, "AEON build completed");
    }

    #[tokio::test]
    async fn sync_email_account_rejects_non_loopback_clients() {
        let dir = temp_dir();
        let state = test_state(&dir);

        let result = sync_email_account(
            ConnectInfo("192.168.1.20:53422".parse().unwrap()),
            AxumPath("work-mail".to_string()),
            State(state),
            Json(SyncEmailAccountPayload {
                session_id: "session".to_string(),
                limit: Some(10),
            }),
        )
        .await;

        assert_eq!(result.unwrap_err(), StatusCode::FORBIDDEN);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn sync_email_account_requires_unlock_session() {
        let dir = temp_dir();
        let state = test_state(&dir);

        let result = sync_email_account(
            ConnectInfo("127.0.0.1:53422".parse().unwrap()),
            AxumPath("work-mail".to_string()),
            State(state),
            Json(SyncEmailAccountPayload {
                session_id: "missing-session".to_string(),
                limit: Some(10),
            }),
        )
        .await;

        assert_eq!(result.unwrap_err(), StatusCode::UNAUTHORIZED);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn oauth_access_token_resolution_refreshes_expiring_token() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let token_url = format!("http://{}/token", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 1024];
            loop {
                let n = stream.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
                if request.windows(4).any(|window| window == b"\r\n\r\n")
                    && String::from_utf8_lossy(&request).contains("scope=Mail.Read")
                {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&request);
            assert!(request.contains("grant_type=refresh_token"));
            assert!(request.contains("refresh_token=old-refresh"));
            assert!(request.contains("client_id=client-id"));

            let body = r#"{"access_token":"fresh-access","expires_in":3600}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });

        let dir = temp_dir();
        let state = test_state(&dir);
        {
            let mut vault = state.credential_vault.lock().await;
            vault
                .add_entry(
                    "correct horse",
                    crate::vault::CredentialEntryInput {
                        id: "outlook-oauth".to_string(),
                        kind: crate::vault::CredentialKind::OAuthToken,
                        label: "Outlook OAuth".to_string(),
                        domains: vec!["graph.microsoft.com".to_string()],
                        last_used: 0,
                        auto_fill: false,
                        secret: crate::vault::CredentialSecret::OAuthToken {
                            access_token: "old-access".to_string(),
                            refresh_token: "old-refresh".to_string(),
                            expires_at: 120_000,
                            scopes: vec!["Mail.Read".to_string()],
                            token_url: Some(token_url),
                            client_id: Some("client-id".to_string()),
                            client_secret: None,
                        },
                    },
                )
                .unwrap();
        }
        let key = state
            .credential_vault
            .lock()
            .await
            .unlock_key("correct horse")
            .unwrap();

        let access_token =
            resolve_oauth_access_token_for_sync(&state, key, "outlook-oauth", 61_000)
                .await
                .unwrap();

        assert_eq!(access_token, "fresh-access");
        server.await.unwrap();
        assert_eq!(
            state
                .credential_vault
                .lock()
                .await
                .oauth_access_token_with_key(&key, "outlook-oauth")
                .unwrap(),
            "fresh-access"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    async fn read_imap_command(
        reader: &mut tokio::io::BufReader<tokio::net::tcp::OwnedReadHalf>,
    ) -> String {
        use tokio::io::AsyncBufReadExt;

        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        line.trim_end_matches(['\r', '\n']).to_string()
    }

    #[tokio::test]
    async fn sync_email_account_imports_messages_from_imap_with_password_credential() {
        use tokio::io::AsyncWriteExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = tokio::io::BufReader::new(reader);

            writer.write_all(b"* OK AEON test IMAP\r\n").await.unwrap();
            assert_eq!(
                read_imap_command(&mut reader).await,
                "A0001 LOGIN \"wc@example.test\" \"imap-secret\""
            );
            writer
                .write_all(b"A0001 OK LOGIN completed\r\n")
                .await
                .unwrap();
            assert_eq!(
                read_imap_command(&mut reader).await,
                "A0002 SELECT \"INBOX\""
            );
            writer
                .write_all(b"A0002 OK SELECT completed\r\n")
                .await
                .unwrap();
            assert_eq!(read_imap_command(&mut reader).await, "A0003 UID SEARCH ALL");
            writer
                .write_all(b"* SEARCH 7\r\nA0003 OK SEARCH completed\r\n")
                .await
                .unwrap();
            assert_eq!(
                read_imap_command(&mut reader).await,
                "A0004 UID FETCH 7 (BODY.PEEK[])"
            );
            let raw_message = concat!(
                "Message-ID: <imap-7@example.test>\r\n",
                "From: Sender <sender@example.test>\r\n",
                "To: wc@example.test\r\n",
                "Subject: IMAP imported\r\n",
                "Date: Thu, 01 Jan 1970 00:00:02 +0000\r\n",
                "\r\n",
                "Imported through IMAP.\r\n"
            );
            writer
                .write_all(
                    format!(
                        "* 1 FETCH (UID 7 BODY[] {{{}}}\r\n{} )\r\nA0004 OK FETCH completed\r\n",
                        raw_message.len(),
                        raw_message
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            assert_eq!(read_imap_command(&mut reader).await, "A0005 LOGOUT");
            writer
                .write_all(b"* BYE\r\nA0005 OK LOGOUT completed\r\n")
                .await
                .unwrap();
        });

        let dir = temp_dir();
        let state = test_state(&dir);
        {
            let mut email = state.email_sync.lock().await;
            let mut configured = account();
            configured.provider = EmailProviderConfig::Imap {
                host: "127.0.0.1".to_string(),
                port,
                tls: false,
                mailbox: "INBOX".to_string(),
            };
            configured.credential_ref = Some("imap-password".to_string());
            email.upsert_account(configured).unwrap();
        }
        let key = {
            let mut vault = state.credential_vault.lock().await;
            vault
                .add_entry(
                    "correct horse",
                    crate::vault::CredentialEntryInput {
                        id: "imap-password".to_string(),
                        kind: crate::vault::CredentialKind::Password,
                        label: "IMAP Password".to_string(),
                        domains: vec!["127.0.0.1".to_string()],
                        last_used: 0,
                        auto_fill: false,
                        secret: crate::vault::CredentialSecret::Password {
                            username: "wc@example.test".to_string(),
                            password: "imap-secret".to_string(),
                        },
                    },
                )
                .unwrap();
            vault.unlock_key("correct horse").unwrap()
        };
        let unlocked = state
            .vault_sessions
            .lock()
            .await
            .unlock(key, None, now_ms());

        let response = sync_email_account(
            ConnectInfo("127.0.0.1:53422".parse().unwrap()),
            AxumPath("work-mail".to_string()),
            State(state.clone()),
            Json(SyncEmailAccountPayload {
                session_id: unlocked.session_id,
                limit: Some(5),
            }),
        )
        .await
        .unwrap()
        .0;

        assert_eq!(response.imported.len(), 1);
        assert_eq!(response.imported[0].uid, 7);
        assert_eq!(
            response.imported[0].message_id,
            "imap:<imap-7@example.test>"
        );
        assert_eq!(response.cursor.last_uid, Some(7));
        server.await.unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }
}
