use std::{collections::HashMap, fs::read_dir, path::PathBuf};

use chrono::Utc;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::{
    crypto::{decrypt, encrypt},
    Account, AccountFilter, AccountId, AccountSecret, IntoSafe, SafeAccount, SafeVault, Vault,
    VaultError, VaultId,
};

/// Holds the decrypted vault and its master password in memory.
/// Implements `Drop` to automatically wipe the password from RAM
/// when this vault is locked or the app closes.
pub struct UnlockedVault {
    pub vault: Vault,
    master_password: String,
}

impl Drop for UnlockedVault {
    fn drop(&mut self) {
        self.master_password.zeroize();
    }
}

pub struct VaultManager {
    /// Where to store vault files
    vaults_dir: PathBuf,

    /// A map of currently unlocked vaults + their master passwords
    unlocked_vaults: HashMap<VaultId, UnlockedVault>,
}

impl VaultManager {
    /// Create new vault dir
    pub fn new(app_data_dir: PathBuf) -> Result<Self, VaultError> {
        let vaults_dir = app_data_dir.join("vaults");

        if !vaults_dir.exists() {
            std::fs::create_dir_all(&vaults_dir)?;
        }

        Ok(Self {
            vaults_dir,
            unlocked_vaults: HashMap::new(),
        })
    }

    /// Get path to a vault file
    fn vault_path(&self, vault_id: VaultId) -> PathBuf {
        self.vaults_dir.join(format!("{}.vault", vault_id))
    }

    /// Check if ANY vault is unlocked
    pub fn is_any_unlocked(&self) -> bool {
        !self.unlocked_vaults.is_empty()
    }

    /// Check if a SPECIFIC vault is unlocked
    pub fn is_vault_unlocked(&self, vault_id: VaultId) -> bool {
        self.unlocked_vaults.contains_key(&vault_id)
    }

    /// Get an immutable reference to a specific unlocked vault
    fn get_vault(&self, vault_id: VaultId) -> Result<&Vault, VaultError> {
        self.unlocked_vaults
            .get(&vault_id)
            .map(|uv| &uv.vault)
            .ok_or(VaultError::VaultLocked)
    }

    /// Get a mutable reference to a specific unlocked vault
    fn get_vault_mut(&mut self, vault_id: VaultId) -> Result<&mut Vault, VaultError> {
        self.unlocked_vaults
            .get_mut(&vault_id)
            .map(|uv| &mut uv.vault)
            .ok_or(VaultError::VaultLocked)
    }

    /// Returns a list of existing vault IDs (filenames) without unlocking them
    pub fn list_vault_ids(&self) -> Result<Vec<String>, VaultError> {
        let mut ids = Vec::new();

        if !self.vaults_dir.exists() {
            return Ok(ids);
        }

        for entry in read_dir(&self.vaults_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(extension) = path.extension() {
                if extension == "vault" {
                    if let Some(stem) = path.file_stem() {
                        if let Some(id_str) = stem.to_str() {
                            ids.push(id_str.to_string());
                        }
                    }
                }
            }
        }

        Ok(ids)
    }

    /// Returns a list of all currently unlocked vaults
    pub fn get_unlocked_vaults(&self) -> Vec<SafeVault> {
        self.unlocked_vaults
            .values()
            .map(|uv| uv.vault.into_safe())
            .collect()
    }

    /// Create a new vault and save to disk
    pub fn create_vault(
        &mut self,
        name: String,
        master_password: String,
    ) -> Result<Vault, VaultError> {
        use uuid::Uuid;

        let vault = Vault {
            id: Uuid::new_v4(),
            name: name.clone(),
            // Color Accent Muted
            color: "#6240BF".to_string(),
            accounts: Vec::new(),
        };

        let json_bytes = serde_json::to_vec(&vault)?;
        let encrypted_string = encrypt(&master_password, &json_bytes)?;

        let path = self.vault_path(vault.id);
        std::fs::write(&path, encrypted_string)?;

        self.unlocked_vaults.insert(
            vault.id,
            UnlockedVault {
                vault: vault.clone(),
                master_password,
            },
        );

        Ok(vault)
    }

    /// Lock a specific vault (removes from RAM and zeroes password)
    /// If vault_id is None, lock ALL vaults.
    pub fn lock_vault(&mut self, vault_id: Option<VaultId>) {
        if let Some(id) = vault_id {
            // Removing from HashMap triggers the `Drop` trait on UnlockedVault,
            // which automatically zeroizes the master password!
            self.unlocked_vaults.remove(&id);
        } else {
            self.unlocked_vaults.clear();
        }
    }

    /// Load vault file from a given id
    pub fn unlock_vault(
        &mut self,
        vault_id: VaultId,
        master_password: String,
    ) -> Result<Vault, VaultError> {
        let path = self.vault_path(vault_id);

        if !path.exists() {
            return Err(VaultError::NotFound(vault_id.to_string()));
        }

        let encrypted_string = std::fs::read_to_string(&path)?;
        let json_bytes = decrypt(&master_password, &encrypted_string)?;

        let vault: Vault = serde_json::from_slice(&json_bytes)?;

        self.unlocked_vaults.insert(
            vault.id,
            UnlockedVault {
                vault: vault.clone(),
                master_password,
            },
        );

        Ok(vault)
    }

    /// Save a specific vault to disk
    fn save_vault(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let unlocked = self
            .unlocked_vaults
            .get(&vault_id)
            .ok_or(VaultError::VaultLocked)?;

        let json_bytes = serde_json::to_vec_pretty(&unlocked.vault)?;
        let encrypted_string = encrypt(&unlocked.master_password, &json_bytes)?;

        let path = self.vault_path(vault_id);
        std::fs::write(path, encrypted_string)?;

        Ok(())
    }

    // Update the currently active vault
    pub fn update_vault(
        &mut self,
        vault_id: VaultId,
        name: Option<String>,
        color: Option<String>,
    ) -> Result<(), VaultError> {
        let vault = self.get_vault_mut(vault_id)?;

        if let Some(n) = name {
            vault.name = n;
        }

        if let Some(c) = color {
            vault.color = c;
        }

        self.save_vault(vault_id)
    }

    /// Create a new service account
    pub fn add_account(
        &mut self,
        vault_id: VaultId,
        display_name: Option<String>,
        username: String,
        email: Option<String>,
        password: String,
    ) -> Result<Account, VaultError> {
        let vault = self.get_vault_mut(vault_id)?;

        let now = Utc::now();
        let account = Account {
            id: Uuid::new_v4(),
            vault_id: vault_id,
            display_name: display_name.filter(|s| !s.is_empty()),
            username,
            email: email.filter(|s| !s.is_empty()),
            favourite: false,
            tags: Vec::new(),
            icon: None,
            color: String::new(),
            secret: AccountSecret {
                id: Uuid::new_v4(),
                password,
            },
            created_at: now,
            updated_at: now,
        };

        vault.accounts.push(account.clone());
        self.save_vault(vault_id)?;

        Ok(account)
    }

    /// Update a service account
    pub fn update_account(
        &mut self,
        vault_id: VaultId,
        account_id: AccountId,
        display_name: Option<String>,
        username: Option<String>,
        email: Option<String>,
        favourite: Option<bool>,
        tags: Option<Vec<String>>,
        icon: Option<String>,
        color: Option<String>,
        password: Option<String>,
    ) -> Result<Account, VaultError> {
        let vault = self.get_vault_mut(vault_id)?;

        let account = vault
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or(VaultError::AccountNotFound(account_id.to_string()))?;

        if let Some(un) = username {
            account.username = un;
        }
        if let Some(pw) = password {
            account.secret.password = pw;
        }
        if let Some(fav) = favourite {
            account.favourite = fav;
        }
        if let Some(t) = tags {
            account.tags = t;
        }
        if let Some(c) = color {
            account.color = c;
        }

        account.display_name = display_name.filter(|s| !s.is_empty());
        account.email = email.filter(|s| !s.is_empty());
        account.icon = icon.filter(|s| !s.is_empty());

        account.updated_at = Utc::now();

        let updated = account.clone();

        self.save_vault(vault_id)?;

        Ok(updated)
    }

    // Delete a service account
    pub fn delete_account(
        &mut self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<(), VaultError> {
        let vault = self.get_vault_mut(vault_id)?;

        let exists = vault.accounts.iter().any(|a| a.id == account_id);
        if !exists {
            return Err(VaultError::AccountNotFound(account_id.to_string()));
        }

        vault.accounts.retain(|a| a.id != account_id);

        self.save_vault(vault_id)?;

        Ok(())
    }

    /// Returns only the password string for a specific account
    pub fn get_secret(
        &self,
        vault_id: VaultId,
        account_id: uuid::Uuid,
    ) -> Result<String, VaultError> {
        let vault = self.get_vault(vault_id)?;

        let account = vault
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or(VaultError::AccountNotFound(account_id.to_string()))?;

        Ok(account.secret.password.clone())
    }

    // Retreive a specific account from a given vault.
    pub fn get_account(
        &self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<SafeAccount, VaultError> {
        let vault = self.get_vault(vault_id)?;

        let account = vault
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or(VaultError::AccountNotFound(account_id.to_string()))?;

        Ok(account.into_safe())
    }

    /// Retrieves accounts across one or all vaults, applying filters.
    pub fn get_all_accounts(&self, filter: AccountFilter) -> Result<Vec<SafeAccount>, VaultError> {
        let mut results: Vec<SafeAccount> = Vec::new();

        // Determine which vaults to scan
        let vaults_to_scan: Vec<&Vault> = if let Some(id) = filter.vault_id {
            // Scan specific vault
            vec![self.get_vault(id)?]
        } else {
            // Scan all unlocked vaults
            self.unlocked_vaults.values().map(|uv| &uv.vault).collect()
        };

        // Prepare optional lowercase search query once for performance
        let query = filter.search_query.map(|q| q.to_lowercase());

        // Iterate and filter
        for vault in vaults_to_scan {
            for account in &vault.accounts {
                // Favourite Filter
                if let Some(true) = filter.favourite_only {
                    if !account.favourite {
                        continue;
                    }
                }

                // Tag Filter (Account must have AT LEAST ONE matching tag)
                if let Some(ref filter_tags) = filter.tags {
                    if !filter_tags.is_empty() {
                        let has_match = filter_tags.iter().any(|ft| account.tags.contains(ft));
                        if !has_match {
                            continue;
                        }
                    }
                }

                // Search Query Filter
                if let Some(ref q) = query {
                    let matches_username = account.username.to_lowercase().contains(q);
                    let matches_display = account
                        .display_name
                        .as_ref()
                        .map_or(false, |dn| dn.to_lowercase().contains(q));
                    let matches_email = account
                        .email
                        .as_ref()
                        .map_or(false, |em| em.to_lowercase().contains(q));

                    if !matches_username && !matches_display && !matches_email {
                        continue;
                    }
                }

                results.push(account.into_safe());
            }
        }

        // Sort by most recently updated first
        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to clean up temp dirs
    fn setup(name: &str) -> PathBuf {
        let temp_dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&temp_dir); // clean up from previous runs
        temp_dir
    }

    #[test]
    fn test_create_and_unlock() {
        let temp_dir = setup("vault_test_enc");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();

        // Create
        let created = manager
            .create_vault("My Vault".to_string(), master_pw.clone())
            .unwrap();
        assert_eq!(created.name, "My Vault");
        assert_eq!(created.color, "#6240BF");

        // Lock
        manager.lock_vault(Some(created.id));
        assert!(!manager.is_vault_unlocked(created.id));

        // Unlock
        let unlocked = manager.unlock_vault(created.id, master_pw).unwrap();
        assert_eq!(unlocked.id, created.id);
        assert!(manager.is_vault_unlocked(created.id));

        // Clean up
        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_add_account() {
        let temp_dir = std::env::temp_dir().join("vault_test_account");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager.create_vault("Test".to_string(), master_pw).unwrap();

        let account = manager
            .add_account(
                vault.id,
                Some("octocat pass".to_string()),
                "octocat".to_string(),
                None,
                "secret123".to_string(),
            )
            .unwrap();

        assert_eq!(account.display_name, Some("octocat pass".to_string()));
        assert_eq!(account.username, "octocat");
        assert_eq!(account.email, None);
        assert_eq!(account.secret.password, "secret123");
        assert_eq!(account.favourite, false);
        assert_eq!(account.tags.len(), 0);

        // Verify it persisted
        let vault = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault.accounts.len(), 1);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_update_account() {
        let temp_dir = setup("vault_test_update_flat");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager.create_vault("Test".to_string(), master_pw).unwrap();

        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                "secret123".to_string(),
            )
            .unwrap();

        let updated = manager
            .update_account(
                vault.id,
                account.id,
                None,
                Some("octocat".to_string()),
                Some("hello@example.com".to_string()),
                Some(true),
                Some(vec!["social".to_string(), "work".to_string()]),
                None,
                None,
                Some("new_pass".to_string()),
            )
            .unwrap();

        assert_eq!(updated.username, "octocat"); // unchanged
        assert_eq!(updated.email, Some("hello@example.com".to_string()));
        assert_eq!(updated.secret.password, "new_pass"); // changed
        assert_eq!(updated.favourite, true);
        assert_eq!(updated.tags, vec!["social".to_string(), "work".to_string()]);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_delete_account() {
        let temp_dir = std::env::temp_dir().join("vault_test_delete");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager.create_vault("Test".to_string(), master_pw).unwrap();

        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                "pass".to_string(),
            )
            .unwrap();

        manager.delete_account(vault.id, account.id).unwrap();

        let vault = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault.accounts.len(), 0);

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
