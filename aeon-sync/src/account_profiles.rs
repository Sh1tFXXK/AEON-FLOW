use crate::server::AppState;
use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedAccount {
    pub id: String,
    pub provider: AccountProvider,
    pub label: String,
    pub credential_ref: Option<String>,
    pub browser_profile: Option<BrowserProfile>,
    pub sharing: SharingPolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountProvider {
    Google,
    Apple,
    Microsoft,
    WeChat,
    Telegram,
    Twitter,
    GitHub,
    Custom {
        name: String,
        auth_url: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserProfile {
    pub profile_dir: PathBuf,
    pub extension_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SharingPolicy {
    pub shared_data: Vec<AccountDataType>,
    pub isolated_data: Vec<AccountDataType>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccountDataType {
    Contacts,
    Bookmarks,
    History,
    Cookies,
    LocalStorage,
    Downloads,
    Passwords,
    Extensions,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserLaunchPlan {
    pub executable: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug)]
pub struct AccountProfileStore {
    path: PathBuf,
    accounts: Vec<ManagedAccount>,
}

#[derive(Debug, Deserialize)]
pub struct BrowserPlanPayload {
    pub executable: Option<PathBuf>,
}

impl Default for SharingPolicy {
    fn default() -> Self {
        Self {
            shared_data: Vec::new(),
            isolated_data: vec![
                AccountDataType::Cookies,
                AccountDataType::LocalStorage,
                AccountDataType::Passwords,
            ],
        }
    }
}

impl ManagedAccount {
    pub fn browser_launch_plan(&self, executable: PathBuf) -> Option<BrowserLaunchPlan> {
        let profile = self.browser_profile.as_ref()?;
        let mut args = vec![
            format!("--user-data-dir={}", profile.profile_dir.display()),
            "--profile-directory=Default".to_string(),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
        ];
        if let Some(extension_dir) = &profile.extension_dir {
            args.push(format!("--load-extension={}", extension_dir.display()));
        }
        Some(BrowserLaunchPlan { executable, args })
    }
}

impl AccountProfileStore {
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let accounts = read_accounts(&path)?;
        Ok(Self { path, accounts })
    }

    pub fn list(&self) -> Vec<ManagedAccount> {
        self.accounts.clone()
    }

    pub fn upsert(&mut self, account: ManagedAccount) -> io::Result<()> {
        if let Some(existing) = self
            .accounts
            .iter_mut()
            .find(|existing| existing.id == account.id)
        {
            *existing = account;
        } else {
            self.accounts.push(account);
        }
        self.accounts.sort_by(|a, b| a.id.cmp(&b.id));
        write_accounts(&self.path, &self.accounts)
    }

    pub fn browser_launch_plan(
        &self,
        account_id: &str,
        executable: PathBuf,
    ) -> Option<BrowserLaunchPlan> {
        self.accounts
            .iter()
            .find(|account| account.id == account_id)
            .and_then(|account| account.browser_launch_plan(executable))
    }
}

pub async fn list_accounts(State(state): State<AppState>) -> Json<Vec<ManagedAccount>> {
    Json(state.account_profiles.lock().await.list())
}

pub async fn upsert_account(
    State(state): State<AppState>,
    Json(account): Json<ManagedAccount>,
) -> Result<Json<Vec<ManagedAccount>>, StatusCode> {
    let mut store = state.account_profiles.lock().await;
    store
        .upsert(account)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(store.list()))
}

pub async fn browser_launch_plan(
    AxumPath(account_id): AxumPath<String>,
    State(state): State<AppState>,
    Json(payload): Json<BrowserPlanPayload>,
) -> Result<Json<BrowserLaunchPlan>, StatusCode> {
    let executable = payload
        .executable
        .unwrap_or_else(|| PathBuf::from("chrome.exe"));
    let store = state.account_profiles.lock().await;
    let plan = store
        .browser_launch_plan(&account_id, executable)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(plan))
}

fn read_accounts(path: &Path) -> io::Result<Vec<ManagedAccount>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(path)?;
    let mut accounts = serde_json::from_slice::<Vec<ManagedAccount>>(&bytes).unwrap_or_default();
    accounts.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(accounts)
}

fn write_accounts(path: &Path, accounts: &[ManagedAccount]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(accounts)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    std::fs::write(path, bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "aeon-account-profile-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn account(id: &str) -> ManagedAccount {
        ManagedAccount {
            id: id.to_string(),
            provider: AccountProvider::Google,
            label: "Work".to_string(),
            credential_ref: Some("vault-google-work".to_string()),
            browser_profile: Some(BrowserProfile {
                profile_dir: PathBuf::from("E:/profiles/work"),
                extension_dir: Some(PathBuf::from("E:/aeon-extension")),
            }),
            sharing: SharingPolicy::default(),
        }
    }

    #[test]
    fn default_policy_isolates_sensitive_browser_state() {
        let policy = SharingPolicy::default();

        assert!(policy.shared_data.is_empty());
        assert!(policy.isolated_data.contains(&AccountDataType::Cookies));
        assert!(policy.isolated_data.contains(&AccountDataType::Passwords));
        assert!(policy
            .isolated_data
            .contains(&AccountDataType::LocalStorage));
    }

    #[test]
    fn store_upserts_accounts_by_id() {
        let dir = temp_dir();
        let mut store = AccountProfileStore::new(dir.join("accounts.json")).unwrap();
        let mut updated = account("google-work");
        updated.label = "Updated Work".to_string();

        store.upsert(account("google-work")).unwrap();
        store.upsert(updated.clone()).unwrap();

        assert_eq!(store.list(), vec![updated]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn browser_launch_plan_uses_isolated_profile_and_extension() {
        let managed = account("google-work");

        let plan = managed
            .browser_launch_plan(PathBuf::from("chrome.exe"))
            .unwrap();

        assert_eq!(plan.executable, PathBuf::from("chrome.exe"));
        assert!(plan
            .args
            .contains(&"--user-data-dir=E:/profiles/work".to_string()));
        assert!(plan
            .args
            .contains(&"--profile-directory=Default".to_string()));
        assert!(plan
            .args
            .contains(&"--load-extension=E:/aeon-extension".to_string()));
    }

    #[test]
    fn store_round_trips_accounts() {
        let dir = temp_dir();
        let path = dir.join("accounts.json");
        let mut store = AccountProfileStore::new(&path).unwrap();
        store.upsert(account("google-work")).unwrap();

        let restored = AccountProfileStore::new(&path).unwrap();

        assert_eq!(restored.list()[0].id, "google-work");
        let _ = std::fs::remove_dir_all(dir);
    }
}
