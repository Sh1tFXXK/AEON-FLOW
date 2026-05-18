use crate::server::AppState;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use pbkdf2::pbkdf2_hmac;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::io;
use std::path::{Path, PathBuf};

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;
const DEFAULT_ITERATIONS: u32 = 120_000;

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

#[derive(Debug)]
pub struct CredentialVaultStore {
    path: PathBuf,
    file: VaultFile,
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
    #[cfg(test)]
    MissingEntry,
    #[cfg(test)]
    DecryptFailed,
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

    #[cfg(test)]
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
        let decrypted = decrypt(&key, &entry.nonce, &entry.encrypted_data)?;
        serde_json::from_slice(&decrypted).map_err(|_| VaultError::DecryptFailed)
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

#[cfg(test)]
fn decrypt(key: &[u8; KEY_LEN], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, VaultError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| VaultError::CryptoUnavailable)?;
    let nonce = Nonce::from_slice(nonce);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| VaultError::DecryptFailed)
}

fn random_bytes(len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

impl From<io::Error> for VaultError {
    fn from(_: io::Error) -> Self {
        VaultError::Io
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
            auto_fill: false,
            secret: CredentialSecret::Password {
                username: "wc@example.test".to_string(),
                password: "super-secret".to_string(),
            },
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
}
