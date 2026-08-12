use std::{collections::HashMap, fs::read_dir, path::PathBuf};

use chrono::Utc;
use uuid::Uuid;
use zxcvbn::{zxcvbn, Entropy};

use crate::{
    crypto::{
        decrypt_with_key, derive_master_key, encrypt_with_key, generate_salt, MasterKey, SALT_LEN,
    },
    Account, AccountFilter, AccountId, IntoSafe, SafeAccount, SafeVault, Vault, VaultError,
    VaultId,
};

/// An unlocked vault held in RAM, along with its cached encryption key,
/// salt, and dirty flag.
///
/// When this struct is dropped, `MasterKey` is automatically zeroized.
struct UnlockedVault {
    vault: Vault,
    master_key: MasterKey,
    salt: [u8; SALT_LEN],
    dirty: bool,
}

pub struct VaultManager {
    /// Where to store vault files
    vaults_dir: PathBuf,

    /// A map of decrypted unlocked vaults (with cached keys)
    unlocked_vaults: HashMap<VaultId, UnlockedVault>,

    /// When true (default), mutations save immediately.
    /// When false, mutations only mark dirty — caller must flush.
    autosave: bool,
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
            autosave: true,
        })
    }

    /// Toggle autosave. When disabled, mutations only mark vaults dirty.
    /// Call `flush_all()` (or `flush_vault`) to persist changes.
    pub fn set_autosave(&mut self, enabled: bool) {
        self.autosave = enabled;
    }

    /// Get path to a vault file
    fn vault_path(&self, vault_id: VaultId) -> PathBuf {
        self.vaults_dir.join(format!("{}.vault", vault_id))
    }

    /// Check if a SPECIFIC vault is unlocked
    fn is_vault_unlocked(&self, vault_id: VaultId) -> bool {
        self.unlocked_vaults.contains_key(&vault_id)
    }

    /// Get an immutable reference to a specific unlocked vault's data
    pub fn get_vault(&self, vault_id: VaultId) -> Result<&Vault, VaultError> {
        self.unlocked_vaults
            .get(&vault_id)
            .map(|uv| &uv.vault)
            .ok_or(VaultError::VaultLocked)
    }

    /// Get a mutable reference to a specific unlocked vault's data.
    /// Also marks the vault as dirty.
    fn get_vault_mut(&mut self, vault_id: VaultId) -> Result<&mut Vault, VaultError> {
        let uv = self
            .unlocked_vaults
            .get_mut(&vault_id)
            .ok_or(VaultError::VaultLocked)?;
        uv.dirty = true;
        Ok(&mut uv.vault)
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

    /// Create a new vault, derive master key (Argon2id — expensive),
    /// cache it, and save to disk.
    pub fn create_vault(
        &mut self,
        name: String,
        master_password: &str,
        secret_key: &[u8],
    ) -> Result<Vault, VaultError> {
        let vault = Vault {
            id: Uuid::new_v4(),
            name: name.clone(),
            // Color Accent Muted
            color: "#6240BF".to_string(),
            accounts: Vec::new(),
        };

        let salt = generate_salt();
        let master_key = derive_master_key(master_password, secret_key, &salt)?;

        self.unlocked_vaults.insert(
            vault.id,
            UnlockedVault {
                vault: vault.clone(),
                master_key,
                salt,
                dirty: false,
            },
        );

        // Initial save (uses cached key — no re-derivation)
        self.save_vault(vault.id)?;

        Ok(vault)
    }

    /// Lock a specific vault (drops from RAM, zeroizes the master key).
    /// If vault_id is None, lock ALL vaults.
    pub fn lock_vault(&mut self, vault_id: Option<VaultId>) {
        if let Some(id) = vault_id {
            // Removing from HashMap drops UnlockedVault, which drops
            // MasterKey, which zeroizes the key bytes.
            self.unlocked_vaults.remove(&id);
        } else {
            self.unlocked_vaults.clear();
        }
    }

    /// Load vault file from disk, derive master key (Argon2id — expensive),
    /// cache it, and decrypt the vault contents.
    pub fn unlock_vault(
        &mut self,
        vault_id: VaultId,
        master_password: &str,
        secret_key: &[u8],
    ) -> Result<Vault, VaultError> {
        let path = self.vault_path(vault_id);

        if !path.exists() {
            return Err(VaultError::NotFound(vault_id.to_string()));
        }

        let encrypted_string = std::fs::read_to_string(&path)?;

        // Extract salt from the file so we can derive the key
        let salt = crate::crypto::extract_salt(&encrypted_string)?;

        // Derive master key (expensive — this is the only Argon2id call
        // for this vault until it's locked again)
        let master_key = derive_master_key(master_password, secret_key, &salt)?;

        // Decrypt using the freshly derived key
        let (json_bytes, salt_from_payload) = decrypt_with_key(&master_key, &encrypted_string)?;
        debug_assert_eq!(salt, salt_from_payload);

        let vault: Vault = serde_json::from_slice(&json_bytes)?;

        self.unlocked_vaults.insert(
            vault.id,
            UnlockedVault {
                vault: vault.clone(),
                master_key,
                salt,
                dirty: false,
            },
        );

        Ok(vault)
    }

    /// Save a specific vault to disk using its cached master key.
    /// This is FAST. no Argon2id, just AES-GCM + disk write.
    fn save_vault(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        // Borrow immutably to read data needed for encryption
        let (_, encrypted) = {
            let uv = self
                .unlocked_vaults
                .get(&vault_id)
                .ok_or(VaultError::VaultLocked)?;

            // Compact serialization (faster + smaller than to_vec_pretty)
            let json_bytes = serde_json::to_vec(&uv.vault)?;

            let encrypted = encrypt_with_key(&uv.master_key, &uv.salt, &json_bytes)?;
            (json_bytes, encrypted)
        };
        // Immutable borrow ends here — NLL allows mutable borrow below

        let path = self.vault_path(vault_id);
        std::fs::write(path, encrypted)?;

        // Clear dirty flag
        if let Some(uv) = self.unlocked_vaults.get_mut(&vault_id) {
            uv.dirty = false;
        }

        Ok(())
    }

    /// Explicitly flush a specific vault to disk if it's dirty.
    /// Returns Ok(()) even if the vault wasn't dirty.
    pub fn flush_vault(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        let is_dirty = self
            .unlocked_vaults
            .get(&vault_id)
            .map(|uv| uv.dirty)
            .unwrap_or(false);

        if is_dirty {
            self.save_vault(vault_id)?;
        }
        Ok(())
    }

    /// Flush ALL dirty vaults to disk. Useful before closing / backgrounding.
    pub fn flush_all(&mut self) -> Result<(), VaultError> {
        let dirty_ids: Vec<VaultId> = self
            .unlocked_vaults
            .iter()
            .filter(|(_, uv)| uv.dirty)
            .map(|(id, _)| *id)
            .collect();

        for id in dirty_ids {
            self.save_vault(id)?;
        }

        Ok(())
    }

    /// Check if a vault has unsaved changes
    pub fn is_dirty(&self, vault_id: VaultId) -> bool {
        self.unlocked_vaults
            .get(&vault_id)
            .map(|uv| uv.dirty)
            .unwrap_or(false)
    }

    /// Update a specific vault's metadata. Saves immediately if autosave
    /// is enabled; otherwise marks dirty.
    pub fn update_vault(
        &mut self,
        vault_id: VaultId,
        name: Option<String>,
        color: Option<String>,
    ) -> Result<(), VaultError> {
        {
            let vault = self.get_vault_mut(vault_id)?;
            if let Some(n) = name {
                vault.name = n;
            }
            if let Some(c) = color {
                vault.color = c;
            }
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Delete a given vault file and remove from RAM.
    pub fn delete_vault(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        if self.is_vault_unlocked(vault_id) {
            self.lock_vault(Some(vault_id));
        }

        let path = self.vault_path(vault_id);

        std::fs::remove_file(path).map_err(VaultError::Io)?;

        Ok(())
    }

    /// Create a new service account
    pub fn add_account(
        &mut self,
        vault_id: VaultId,
        display_name: Option<String>,
        username: String,
        email: Option<String>,
        icon: Option<String>,
        password: String,
    ) -> Result<Account, VaultError> {
        let now = Utc::now();
        let account = Account {
            id: Uuid::new_v4(),
            vault_id,
            display_name: display_name.filter(|s| !s.is_empty()),
            username,
            email: email.filter(|s| !s.is_empty()),
            favourite: false,
            tags: Vec::new(),
            icon: icon,
            color: String::new(),
            secret: crate::AccountSecret {
                id: Uuid::new_v4(),
                password,
            },
            created_at: now,
            updated_at: now,
        };

        {
            let vault = self.get_vault_mut(vault_id)?;
            vault.accounts.push(account.clone());
        }

        self.maybe_save(vault_id)?;
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
        let updated = {
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
            if let Some(name) = display_name {
                account.display_name = if name.is_empty() { None } else { Some(name) };
            }
            if let Some(email) = email {
                account.email = if email.is_empty() { None } else { Some(email) };
            }
            if let Some(icon) = icon {
                account.icon = if icon.is_empty() { None } else { Some(icon) };
            }

            account.updated_at = Utc::now();
            account.clone()
        };

        self.maybe_save(vault_id)?;
        Ok(updated)
    }

    /// Delete a service account
    pub fn delete_account(
        &mut self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<(), VaultError> {
        {
            let vault = self.get_vault_mut(vault_id)?;
            let exists = vault.accounts.iter().any(|a| a.id == account_id);
            if !exists {
                return Err(VaultError::AccountNotFound(account_id.to_string()));
            }
            vault.accounts.retain(|a| a.id != account_id);
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Returns only the password string for a specific account
    pub fn get_secret(
        &self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<String, VaultError> {
        let vault = self.get_vault(vault_id)?;

        let account = vault
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or(VaultError::AccountNotFound(account_id.to_string()))?;

        Ok(account.secret.password.clone())
    }

    /// Get the strength of a given accounts password
    pub fn get_account_password_strength(
        &self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<Entropy, VaultError> {
        let vault = self.get_vault(vault_id)?;

        let account = vault
            .accounts
            .iter()
            .find(|a| a.id == account_id)
            .ok_or(VaultError::AccountNotFound(account_id.to_string()))?;

        Ok(zxcvbn(&account.secret.password, &[]))
    }

    /// Retrieve a specific account by id (searches all unlocked vaults)
    pub fn get_account_by_id(&self, account_id: AccountId) -> Result<SafeAccount, VaultError> {
        let account = self
            .unlocked_vaults
            .values()
            .find_map(|uv| uv.vault.accounts.iter().find(|a| a.id == account_id))
            .ok_or_else(|| VaultError::AccountNotFound(account_id.to_string()))?;

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

    /// Save immediately if autosave is on. Otherwise just leave the dirty
    /// flag set (caller is responsible for flushing).
    fn maybe_save(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        if self.autosave {
            self.save_vault(vault_id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> PathBuf {
        let unique_id = Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("vault_manager_test_{}", unique_id));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    const MASTER_PW: &str = "SuperSecret123";
    const KEY: [u8; 32] = [0u8; 32];

    #[test]
    fn test_create_and_unlock() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();

        let created = manager
            .create_vault("My Vault".to_string(), MASTER_PW, &KEY)
            .unwrap();
        assert_eq!(created.name, "My Vault");
        assert_eq!(created.color, "#6240BF");

        manager.lock_vault(Some(created.id));
        assert!(!manager.is_vault_unlocked(created.id));

        let unlocked = manager.unlock_vault(created.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(unlocked.id, created.id);
        assert!(manager.is_vault_unlocked(created.id));
    }

    #[test]
    fn test_add_account() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Note: no more mp/sk arguments needed!
        let account = manager
            .add_account(
                vault.id,
                Some("octocat pass".to_string()),
                "octocat".to_string(),
                None,
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

        let vault = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault.accounts.len(), 1);
    }

    #[test]
    fn test_update_account() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "secret123".to_string(),
            )
            .unwrap();

        // No more mp/sk arguments!
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

        assert_eq!(updated.username, "octocat");
        assert_eq!(updated.email, Some("hello@example.com".to_string()));
        assert_eq!(updated.secret.password, "new_pass");
        assert_eq!(updated.favourite, true);
        assert_eq!(updated.tags, vec!["social".to_string(), "work".to_string()]);
    }

    #[test]
    fn test_delete_account() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "pass".to_string(),
            )
            .unwrap();

        // No more mp/sk arguments!
        manager.delete_account(vault.id, account.id).unwrap();

        let vault = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault.accounts.len(), 0);
    }

    #[test]
    fn test_batch_mode_dirty_tracking() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Disable autosave for batch operations
        manager.set_autosave(false);

        // Add 3 accounts — none should be persisted yet
        for i in 0..3 {
            manager
                .add_account(
                    vault.id,
                    None,
                    format!("user{}", i),
                    None,
                    None,
                    format!("pass{}", i),
                )
                .unwrap();
        }

        assert!(manager.is_dirty(vault.id));

        // Lock and re-unlock — data should NOT be there (wasn't flushed)
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(
            re_unlocked.accounts.len(),
            0,
            "Batch changes were not flushed"
        );

        // Re-add in batch mode
        manager.set_autosave(false);
        for i in 0..3 {
            manager
                .add_account(
                    vault.id,
                    None,
                    format!("user{}", i),
                    None,
                    None,
                    format!("pass{}", i),
                )
                .unwrap();
        }
        assert!(manager.is_dirty(vault.id));

        // Flush explicitly
        manager.flush_vault(vault.id).unwrap();
        assert!(!manager.is_dirty(vault.id));

        // Re-enable autosave
        manager.set_autosave(true);

        // Lock and re-unlock — data SHOULD be there now
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(
            re_unlocked.accounts.len(),
            3,
            "Flushed changes were not persisted"
        );
    }

    #[test]
    fn test_flush_all() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();

        let vault1 = manager
            .create_vault("V1".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault2 = manager
            .create_vault("V2".to_string(), MASTER_PW, &KEY)
            .unwrap();

        manager.set_autosave(false);

        manager
            .add_account(
                vault1.id,
                None,
                "u1".to_string(),
                None,
                None,
                "p1".to_string(),
            )
            .unwrap();
        manager
            .add_account(
                vault2.id,
                None,
                "u2".to_string(),
                None,
                None,
                "p2".to_string(),
            )
            .unwrap();

        assert!(manager.is_dirty(vault1.id));
        assert!(manager.is_dirty(vault2.id));

        manager.flush_all().unwrap();

        assert!(!manager.is_dirty(vault1.id));
        assert!(!manager.is_dirty(vault2.id));

        // Verify persistence
        manager.lock_vault(None);
        let v1 = manager.unlock_vault(vault1.id, MASTER_PW, &KEY).unwrap();
        let v2 = manager.unlock_vault(vault2.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(v1.accounts.len(), 1);
        assert_eq!(v2.accounts.len(), 1);
    }

    #[test]
    fn test_persistence_after_autosave() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Autosave is on by default — add account and verify it persists
        manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "pass".to_string(),
            )
            .unwrap();

        assert!(
            !manager.is_dirty(vault.id),
            "Autosave should have cleared dirty flag"
        );

        // Lock and re-unlock to verify persistence
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(re_unlocked.accounts.len(), 1);
    }
}
