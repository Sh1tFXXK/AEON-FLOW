use aeon_store::{Account, AccountId};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AccountRegistryState {
    default_account: Option<AccountId>,
    accounts: Vec<Account>,
}

#[derive(Debug, Clone)]
pub struct AccountRegistry {
    path: PathBuf,
    state: AccountRegistryState,
}

impl AccountRegistry {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut state = read_state(&path)?;
        normalize_accounts(&mut state.accounts);
        if state
            .default_account
            .is_some_and(|id| !state.accounts.iter().any(|account| account.id == id))
        {
            state.default_account = None;
        }
        Ok(Self { path, state })
    }

    pub fn list(&self) -> &[Account] {
        &self.state.accounts
    }

    pub fn default_account_id(&self) -> Option<AccountId> {
        self.state.default_account
    }

    pub fn default_account(&self) -> Option<&Account> {
        let id = self.state.default_account?;
        self.state.accounts.iter().find(|account| account.id == id)
    }

    pub fn upsert(&mut self, account: Account) -> io::Result<()> {
        match self
            .state
            .accounts
            .iter_mut()
            .find(|existing| existing.id == account.id)
        {
            Some(existing) => *existing = account,
            None => self.state.accounts.push(account),
        }

        normalize_accounts(&mut self.state.accounts);

        if self.state.default_account.is_none() {
            self.state.default_account = self.state.accounts.first().map(|account| account.id);
        }

        self.save()
    }

    pub fn set_default(&mut self, account_id: AccountId) -> io::Result<()> {
        if !self
            .state
            .accounts
            .iter()
            .any(|account| account.id == account_id)
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "account id is not in local registry",
            ));
        }
        self.state.default_account = Some(account_id);
        self.save()
    }

    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&self.state).map_err(io::Error::other)?;
        std::fs::write(&self.path, bytes)
    }
}

pub fn default_account_registry_path() -> PathBuf {
    if let Some(path) = std::env::var_os("AEON_ACCOUNT_REGISTRY") {
        return PathBuf::from(path);
    }

    let root = std::env::var_os("AEON_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    root.join(".aeon").join("accounts.json")
}

fn read_state(path: &Path) -> io::Result<AccountRegistryState> {
    if !path.exists() {
        return Ok(AccountRegistryState::default());
    }

    let bytes = std::fs::read(path)?;
    if bytes.is_empty() {
        return Ok(AccountRegistryState::default());
    }

    serde_json::from_slice(&bytes).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn normalize_accounts(accounts: &mut Vec<Account>) {
    accounts.sort_by(|a, b| a.id.cmp(&b.id));
    accounts.dedup_by(|a, b| a.id == b.id);
}
