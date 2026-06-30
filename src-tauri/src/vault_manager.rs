use std::path::PathBuf;

use uuid::Uuid;

use crate::{Service, Vault, VaultError, VaultId};

pub struct VaultManager {
    /// Where to store vault files
    vaults_dir: PathBuf,

    /// Currently unlocked vault
    unlocked_vault: Option<Vault>,
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

    /// Create a new vault and save to disk
    pub fn create_vault(&mut self, name: String) -> Result<Vault, VaultError> {
        use uuid::Uuid;

        let vault = Vault {
            id: Uuid::new_v4(),
            name: name.clone(),
            services: Vec::new(),
        };

        let path = self.vault_path(vault.id);
        let json = serde_json::to_string_pretty(&vault)?;
        std::fs::write(&path, json)?;

        self.unlocked_vault = Some(vault.clone());

        Ok(vault)
    }

    /// Load vault file from a given id
    pub fn load_vault(&mut self, vault_id: VaultId) -> Result<Vault, VaultError> {
        let path = self.vault_path(vault_id);

        if !path.exists() {
            return Err(VaultError::NotFound(vault_id.to_string()));
        }

        let json = std::fs::read_to_string(&path)?;

        let vault: Vault = serde_json::from_str(&json)?;

        self.unlocked_vault = Some(vault.clone());

        Ok(vault)
    }

    /// Save the currently unlocked vault
    fn save_vault(&self) -> Result<(), VaultError> {
        let vault = self.get_vault()?;
        let path = self.vault_path(vault.id);
        let json = serde_json::to_string_pretty(vault)?;

        std::fs::write(path, json)?;

        Ok(())
    }

    /// Lock the currently unlocked vault
    pub fn lock_vault(&mut self) {
        self.unlocked_vault = None;
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_vault() {
        // Use a temp directory for tests
        let temp_dir = std::env::temp_dir().join("vault_test");

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();

        let vault = manager.create_vault("My Vault".to_string()).unwrap();

        assert_eq!(vault.name, "My Vault");
        assert_eq!(vault.services.len(), 0);
        assert!(manager.is_unlocked());

        // Verify file was created
        let path = temp_dir.join("vaults").join(format!("{}.vault", vault.id));
        assert!(path.exists());

        // Clean up
        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_load_vault() {
        let temp_dir = std::env::temp_dir().join("vault_test_load");

        // Create a vault
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let created = manager.create_vault("Test Vault".to_string()).unwrap();

        // Lock it (clear from memory)
        manager.unlocked_vault = None;
        assert!(!manager.is_unlocked());

        // Load it back
        let loaded = manager.load_vault(created.id).unwrap();

        assert_eq!(loaded.id, created.id);
        assert_eq!(loaded.name, "Test Vault");
        assert!(manager.is_unlocked());

        // Clean up
        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_add_service() {
        let temp_dir = std::env::temp_dir().join("vault_test_service");

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.create_vault("Test".to_string()).unwrap();

        let service = manager.add_service("GitHub".to_string()).unwrap();

        assert_eq!(service.name, "GitHub");
        assert_eq!(service.accounts.len(), 0);

        // Verify it's in the vault
        let vault = manager.get_vault().unwrap();
        assert_eq!(vault.services.len(), 1);

        // Clean up
        std::fs::remove_dir_all(temp_dir).ok();
    }
}
