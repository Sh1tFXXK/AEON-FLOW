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
    pub account_id: String,
    pub label: String,
    pub credential_ref: Option<String>,
    pub executable: PathBuf,
    pub args: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BrowserPlanError {
    AccountNotFound,
    MissingBrowserProfile,
    UnsupportedUrl,
}

#[derive(Debug)]
pub struct AccountProfileStore {
    path: PathBuf,
    accounts: Vec<ManagedAccount>,
}

#[derive(Debug, Deserialize)]
pub struct BrowserPlanPayload {
    pub executable: Option<PathBuf>,
    pub url: Option<String>,
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
    pub fn browser_launch_plan(
        &self,
        executable: PathBuf,
        target_url: Option<String>,
    ) -> Result<BrowserLaunchPlan, BrowserPlanError> {
        let profile = self
            .browser_profile
            .as_ref()
            .ok_or(BrowserPlanError::MissingBrowserProfile)?;
        let mut args = vec![
            format!("--user-data-dir={}", profile.profile_dir.display()),
            "--profile-directory=Default".to_string(),
            "--no-first-run".to_string(),
            "--no-default-browser-check".to_string(),
        ];
        if let Some(extension_dir) = &profile.extension_dir {
            args.push(format!("--load-extension={}", extension_dir.display()));
        }
        if let Some(target_url) = normalize_browser_target_url(target_url)? {
            args.push(target_url);
        }
        Ok(BrowserLaunchPlan {
            account_id: self.id.clone(),
            label: self.label.clone(),
            credential_ref: self.credential_ref.clone(),
            executable,
            args,
        })
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
        target_url: Option<String>,
    ) -> Result<BrowserLaunchPlan, BrowserPlanError> {
        let account = self
            .accounts
            .iter()
            .find(|account| account.id == account_id)
            .ok_or(BrowserPlanError::AccountNotFound)?;
        account.browser_launch_plan(executable, target_url)
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
        .browser_launch_plan(&account_id, executable, payload.url)
        .map_err(browser_plan_status)?;
    Ok(Json(plan))
}

fn browser_plan_status(err: BrowserPlanError) -> StatusCode {
    match err {
        BrowserPlanError::AccountNotFound | BrowserPlanError::MissingBrowserProfile => {
            StatusCode::NOT_FOUND
        }
        BrowserPlanError::UnsupportedUrl => StatusCode::BAD_REQUEST,
    }
}

fn normalize_browser_target_url(
    target_url: Option<String>,
) -> Result<Option<String>, BrowserPlanError> {
    let Some(target_url) = target_url
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    if target_url.starts_with("https://") || target_url.starts_with("http://") {
        return Ok(Some(target_url));
    }

    Err(BrowserPlanError::UnsupportedUrl)
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
            .browser_launch_plan(PathBuf::from("chrome.exe"), None)
            .unwrap();

        assert_eq!(plan.account_id, "google-work");
        assert_eq!(plan.label, "Work");
        assert_eq!(plan.credential_ref, Some("vault-google-work".to_string()));
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
    fn browser_launch_plan_appends_web_target_url() {
        let managed = account("google-work");

        let plan = managed
            .browser_launch_plan(
                PathBuf::from("chrome.exe"),
                Some("https://example.test/work".to_string()),
            )
            .unwrap();

        assert_eq!(
            plan.args.last(),
            Some(&"https://example.test/work".to_string())
        );
    }

    #[test]
    fn browser_launch_plan_rejects_non_web_target_url() {
        let managed = account("google-work");

        let result = managed.browser_launch_plan(
            PathBuf::from("chrome.exe"),
            Some("file:///C:/Users/Wc/secret.txt".to_string()),
        );

        assert!(matches!(result, Err(BrowserPlanError::UnsupportedUrl)));
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
