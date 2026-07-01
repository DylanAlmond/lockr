use std::{fs::read_dir, path::PathBuf};

use chrono::Utc;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::{
    crypto::{decrypt, encrypt},
    AccountId, Service, ServiceId, Vault, VaultError, VaultId,
};

pub struct VaultManager {
    /// Where to store vault files
    vaults_dir: PathBuf,

    /// Currently unlocked vault
    unlocked_vault: Option<Vault>,

    // Master password for currently unlocked vault
    master_password: Option<String>,
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
            unlocked_vault: None,
            master_password: None,
        })
    }

    /// Get path to a vault file
    fn vault_path(&self, vault_id: VaultId) -> PathBuf {
        self.vaults_dir.join(format!("{}.vault", vault_id))
    }

    /// Check if a vault is unlocked
    pub fn is_unlocked(&self) -> bool {
        self.unlocked_vault.is_some()
    }

    /// Get a reference to the vault, or error if unlocked
    fn get_vault(&self) -> Result<&Vault, VaultError> {
        self.unlocked_vault.as_ref().ok_or(VaultError::VaultLocked)
    }

    /// Get a mutable reference to the vault, or error if unlocked
    fn get_vault_mut(&mut self) -> Result<&mut Vault, VaultError> {
        self.unlocked_vault.as_mut().ok_or(VaultError::VaultLocked)
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
            services: Vec::new(),
        };

        let json_bytes = serde_json::to_vec(&vault)?;
        let encrypted_string = encrypt(&master_password, &json_bytes)?;

        let path = self.vault_path(vault.id);
        std::fs::write(&path, encrypted_string)?;

        self.unlocked_vault = Some(vault.clone());
        self.master_password = Some(master_password);

        Ok(vault)
    }

    /// Lock the currently unlocked vault
    pub fn lock_vault(&mut self) {
        self.unlocked_vault = None;

        // Zeroize master password on drop
        if let Some(mut pw) = self.master_password.take() {
            pw.zeroize();
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

        self.unlocked_vault = Some(vault.clone());
        self.master_password = Some(master_password);

        Ok(vault)
    }

    /// Save the currently unlocked vault
    fn save_vault(&self) -> Result<(), VaultError> {
        let vault = self.get_vault()?;

        let master_password = self
            .master_password
            .as_ref()
            .ok_or(VaultError::VaultLocked)?;

        let json_bytes = serde_json::to_vec_pretty(vault)?;
        let encrypted_string = encrypt(master_password, &json_bytes)?;

        let path = self.vault_path(vault.id);
        std::fs::write(path, encrypted_string)?;

        Ok(())
    }

    /// Create a new service
    pub fn add_service(&mut self, name: String) -> Result<Service, VaultError> {
        let vault = self.get_vault_mut()?;

        let service = Service {
            id: Uuid::new_v4(),
            name: name,
            accounts: Vec::new(),
        };

        vault.services.push(service.clone());

        self.save_vault()?;
        Ok(service)
    }

    /// Delete an existing service
    pub fn delete_service(&mut self, service_id: ServiceId) -> Result<(), VaultError> {
        let vault = self.get_vault_mut()?;

        let len_before = vault.services.len();
        vault.services.retain(|s| s.id != service_id);

        if vault.services.len() == len_before {
            return Err(VaultError::ServiceNotFound(service_id.to_string()));
        }

        self.save_vault()?;
        Ok(())
    }

    /// Create a new service account
    pub fn add_account(
        &mut self,
        service_id: ServiceId,
        username: String,
        password: String,
    ) -> Result<crate::models::Account, VaultError> {
        let vault = self.get_vault_mut()?;

        let service = vault
            .find_service_mut(service_id)
            .ok_or(VaultError::ServiceNotFound(service_id.to_string()))?;

        let now = Utc::now();
        let account = crate::models::Account {
            id: Uuid::new_v4(),
            display_name: None,
            username,
            email: None,
            secret: crate::models::AccountSecret {
                id: Uuid::new_v4(),
                password,
            },
            created_at: now,
            updated_at: now,
        };

        service.accounts.push(account.clone());

        self.save_vault()?;

        Ok(account)
    }

    /// Update a service account
    pub fn update_account(
        &mut self,
        service_id: ServiceId,
        account_id: AccountId,
        username: Option<String>,
        password: Option<String>,
    ) -> Result<crate::models::Account, VaultError> {
        let vault = self.get_vault_mut()?;

        let service = vault
            .find_service_mut(service_id)
            .ok_or(VaultError::ServiceNotFound(service_id.to_string()))?;

        let account = service
            .accounts
            .iter_mut()
            .find(|a| a.id == account_id)
            .ok_or(VaultError::AccountNotFound(account_id.to_string()))?;

        if let Some(new_username) = username {
            account.username = new_username;
        }
        if let Some(new_password) = password {
            account.secret.password = new_password;
        }

        account.updated_at = Utc::now();

        let updated = account.clone();

        self.save_vault()?;

        Ok(updated)
    }

    // Delete a service account
    pub fn delete_account(
        &mut self,
        service_id: ServiceId,
        account_id: AccountId,
    ) -> Result<(), VaultError> {
        let vault = self.get_vault_mut()?;

        let service = vault
            .find_service_mut(service_id)
            .ok_or(VaultError::ServiceNotFound(service_id.to_string()))?;

        let exists = service.accounts.iter().any(|a| a.id == account_id);
        if !exists {
            return Err(VaultError::AccountNotFound(account_id.to_string()));
        }

        service.accounts.retain(|a| a.id != account_id);

        self.save_vault()?;

        Ok(())
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

        // Lock
        manager.lock_vault();
        assert!(!manager.is_unlocked());

        // Unlock
        let unlocked = manager.unlock_vault(created.id, master_pw).unwrap();
        assert_eq!(unlocked.id, created.id);
        assert!(manager.is_unlocked());

        // Clean up
        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_add_service() {
        let temp_dir = std::env::temp_dir().join("vault_test_service");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.create_vault("Test".to_string(), master_pw).unwrap();

        let service = manager.add_service("GitHub".to_string()).unwrap();

        assert_eq!(service.name, "GitHub");
        assert_eq!(service.accounts.len(), 0);

        // Verify it's in the vault
        let vault = manager.get_vault().unwrap();
        assert_eq!(vault.services.len(), 1);

        // Clean up
        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_add_account() {
        let temp_dir = std::env::temp_dir().join("vault_test_account");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.create_vault("Test".to_string(), master_pw).unwrap();

        let service = manager.add_service("GitHub".to_string()).unwrap();
        let account = manager
            .add_account(service.id, "octocat".to_string(), "secret123".to_string())
            .unwrap();

        assert_eq!(account.username, "octocat");
        assert_eq!(account.secret.password, "secret123");

        // Verify it persisted
        let vault = manager.get_vault().unwrap();
        assert_eq!(vault.services[0].accounts.len(), 1);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_add_account_wrong_service() {
        let temp_dir = std::env::temp_dir().join("vault_test_wrong");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.create_vault("Test".to_string(), master_pw).unwrap();

        let fake_id = uuid::Uuid::new_v4();
        let result = manager.add_account(fake_id, "user".to_string(), "pass".to_string());

        assert!(result.is_err());
        match result {
            Err(VaultError::ServiceNotFound(_)) => {} // expected
            _ => panic!("Expected ServiceNotFound error"),
        }

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_update_account() {
        let temp_dir = std::env::temp_dir().join("vault_test_update");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.create_vault("Test".to_string(), master_pw).unwrap();
        let service = manager.add_service("GitHub".to_string()).unwrap();
        let account = manager
            .add_account(service.id, "octocat".to_string(), "old_pass".to_string())
            .unwrap();

        // Update only password
        let updated = manager
            .update_account(service.id, account.id, None, Some("new_pass".to_string()))
            .unwrap();

        assert_eq!(updated.username, "octocat"); // unchanged
        assert_eq!(updated.secret.password, "new_pass"); // changed

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_delete_account() {
        let temp_dir = std::env::temp_dir().join("vault_test_delete");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.create_vault("Test".to_string(), master_pw).unwrap();
        let service = manager.add_service("GitHub".to_string()).unwrap();
        let account = manager
            .add_account(service.id, "octocat".to_string(), "pass".to_string())
            .unwrap();

        manager.delete_account(service.id, account.id).unwrap();

        let vault = manager.get_vault().unwrap();
        assert_eq!(vault.services[0].accounts.len(), 0);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_delete_nonexistent_account() {
        let temp_dir = std::env::temp_dir().join("vault_test_delete_missing");
        let master_pw = "SuperSecret123!".to_string();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.create_vault("Test".to_string(), master_pw).unwrap();
        let service = manager.add_service("GitHub".to_string()).unwrap();

        let result = manager.delete_account(service.id, uuid::Uuid::new_v4());
        assert!(matches!(result, Err(VaultError::AccountNotFound(_))));

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
