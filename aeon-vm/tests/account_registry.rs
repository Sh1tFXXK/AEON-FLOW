use aeon_store::Account;
use aeon_vm::account_registry::AccountRegistry;

struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "aeon-vm-account-registry-{}-{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn join(&self, path: &str) -> std::path::PathBuf {
        self.0.join(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn first_account_is_persisted_and_becomes_default() {
    let dir = TempDir::new("first-default");
    let path = dir.join("accounts.json");
    let account = Account::from_public_key("alice", [1u8; 32]);

    let mut registry = AccountRegistry::open(&path).unwrap();
    assert!(registry.list().is_empty());

    registry.upsert(account.clone()).unwrap();

    let reloaded = AccountRegistry::open(&path).unwrap();
    assert_eq!(reloaded.list(), &[account.clone()]);
    assert_eq!(reloaded.default_account(), Some(&account));
}

#[test]
fn upserting_existing_account_replaces_without_changing_default() {
    let dir = TempDir::new("replace");
    let path = dir.join("accounts.json");
    let alice = Account::from_public_key("alice", [1u8; 32]);
    let alice_renamed = Account::from_public_key("alice work", [1u8; 32]);
    let bob = Account::from_public_key("bob", [2u8; 32]);

    let mut registry = AccountRegistry::open(&path).unwrap();
    registry.upsert(alice.clone()).unwrap();
    registry.upsert(bob.clone()).unwrap();
    registry.upsert(alice_renamed.clone()).unwrap();

    let reloaded = AccountRegistry::open(&path).unwrap();
    assert_eq!(reloaded.list().len(), 2);
    assert!(reloaded.list().contains(&alice_renamed));
    assert!(reloaded.list().contains(&bob));
    assert_eq!(reloaded.default_account(), Some(&alice_renamed));
}
