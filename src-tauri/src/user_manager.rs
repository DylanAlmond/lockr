use argon2::password_hash::rand_core::{OsRng, RngCore};
use std::{path::PathBuf, println};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::{
    crypto::{decrypt, encrypt},
    User, UserId, VaultError, VaultId,
};

pub struct UserManager {
    app_data_dir: PathBuf,
    pub active_user_id: Option<UserId>,
    active_path: Option<PathBuf>,
}

impl UserManager {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, VaultError> {
        // Migration for old single-user user.json file
        let old_path = app_data_dir.join("user.json");

        if old_path.exists() {
            let json = std::fs::read_to_string(&old_path)?;
            let user: User = serde_json::from_str(&json)?;

            let new_path = app_data_dir.join(format!("{}.json", user.id));

            // Don't overwrite an existing modern file.
            if !new_path.exists() {
                std::fs::rename(&old_path, &new_path)?;
            }
        }

        Ok(Self {
            app_data_dir,
            active_user_id: None,
            active_path: None,
        })
    }

    // Get all valid accounts listed in app data dir
    pub fn get_users(&self) -> Result<Vec<User>, VaultError> {
        let mut available_accounts: Vec<User> = Vec::new();

        for entry in std::fs::read_dir(&self.app_data_dir)? {
            let path = entry?.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let json = std::fs::read_to_string(&path)?;

            match serde_json::from_str::<User>(&json) {
                Ok(user_data) => {
                    available_accounts.push(user_data);
                }
                Err(err) => {
                    eprintln!("Failed to parse user profile {}: {}", path.display(), err);
                }
            }
        }

        Ok(available_accounts)
    }

    // Check if any registered accounts exist on disk
    pub fn is_registered(&self) -> bool {
        !self.get_users().unwrap_or_default().is_empty()
    }

    /// Check if a user profile already exists on disk
    pub fn is_user_registered(&self, id: &UserId) -> bool {
        self.app_data_dir
            .join(format!("{}.json", id.to_string()))
            .exists()
    }

    /// Create a brand new user profile.
    /// Generates a random Secret Key and encrypts it with the Master Password.
    pub fn register(&mut self, name: String, master_password: &str) -> Result<User, VaultError> {
        let mut secret_key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut secret_key_bytes);

        let encrypted_secret_key = encrypt(master_password, &secret_key_bytes)?;

        // Wipe the raw key from this local scope immediately
        secret_key_bytes.zeroize();

        let user = User {
            id: Uuid::new_v4(),
            name,
            color: "#6240BF".to_string(),
            icon: None,
            encrypted_secret_key,
            vault_ids: Vec::new(),
        };

        self.active_user_id = Some(user.id);
        self.active_path = Some(
            self.app_data_dir
                .join(format!("{}.json", user.id.to_string())),
        );
        self.save_user(&user)?;

        Ok(user)
    }

    /// Verify the Master Password and decrypt the Secret Key.
    pub fn login(&mut self, id: &UserId, master_password: &str) -> Result<Vec<u8>, VaultError> {
        if !self.is_user_registered(id) {
            return Err(VaultError::AccountNotFound("User not registered".into()));
        }

        let path = self.app_data_dir.join(format!("{}.json", id.to_string()));

        let json: String = std::fs::read_to_string(&path)?;

        let user_file: User = serde_json::from_str(&json)?;

        let decrypted_key = decrypt(&master_password, &user_file.encrypted_secret_key)?;

        // Verify it's exactly 32 bytes (sanity check)
        if decrypted_key.len() != 32 {
            return Err(VaultError::Decryption("Invalid secret key length".into()));
        }

        self.active_user_id = Some(user_file.id);
        self.active_path = Some(path);

        Ok(decrypted_key)
    }

    // Logout the active user
    pub fn logout(&mut self) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::AccountNotFound("No user is active".into()))?;

        if !self.is_user_registered(user_id) {
            return Err(VaultError::AccountNotFound("User not registered".into()));
        }

        self.active_user_id = None;
        self.active_path = None;

        Ok(())
    }

    /// Get the user profile metadata (does NOT decrypt the secret key)
    pub fn get_user(&self, id: &UserId) -> Result<User, VaultError> {
        if !self.is_user_registered(id) {
            return Err(VaultError::AccountNotFound("User not registered".into()));
        }

        let path = self
            .active_path
            .as_ref()
            .ok_or(VaultError::AccountNotFound("active_path is not set".into()))?;

        let json: String = std::fs::read_to_string(path)?;
        let user_file: User = serde_json::from_str(&json)?;

        Ok(user_file)
    }

    /// Update active user's profile details (name, color, icon)
    pub fn update_profile(
        &mut self,
        name: Option<String>,
        color: Option<String>,
        icon: Option<String>,
    ) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::AccountNotFound("No user is active".into()))?;

        let mut user = self.get_user(user_id)?;

        if let Some(n) = name {
            user.name = n;
        }
        if let Some(c) = color {
            user.color = c;
        }
        if let Some(i) = icon {
            user.icon = if i.is_empty() { None } else { Some(i) };
        }

        self.save_user(&user)
    }

    /// Add a newly created vault to the active user's list
    pub fn add_vault_to_user(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::AccountNotFound("No user is active".into()))?;

        let mut user = self.get_user(user_id)?;
        if !user.vault_ids.contains(&vault_id) {
            user.vault_ids.push(vault_id);
            self.save_user(&user)?;
        }
        Ok(())
    }

    /// Remove a vault from the user's list (e.g., on deletion)
    pub fn remove_vault_from_user(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::AccountNotFound("No user is active".into()))?;

        let mut user = self.get_user(user_id)?;
        user.vault_ids.retain(|id| id != &vault_id);
        self.save_user(&user)?;
        Ok(())
    }

    // Delete the current user
    pub fn delete_user(&mut self) -> Result<(), VaultError> {
        if self.active_user_id.is_none() {
            return Err(VaultError::AccountNotFound("No user is active".into()));
        }

        let path = self
            .active_path
            .as_ref()
            .ok_or(VaultError::NotFound("active_path is not set".into()))?;

        std::fs::remove_file(path).map_err(VaultError::Io)?;

        self.active_user_id = None;
        self.active_path = None;

        Ok(())
    }

    fn save_user(&self, user: &User) -> Result<(), VaultError> {
        let path = self
            .active_path
            .as_ref()
            .ok_or(VaultError::NotFound("active_path is not set".into()))?;

        let json = serde_json::to_string_pretty(&user)?;

        std::fs::write(path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn setup() -> PathBuf {
        // Generate a unique ID for this specific test run
        let unique_id = Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("user_manager_test_{}", unique_id));

        // Clean up if it somehow exists, then create it fresh
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        temp_dir
    }

    #[test]
    fn test_is_registered_and_register() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        // Should not exist initially
        assert!(!manager.is_registered());

        // Register a new user
        let user = manager
            .register("Alice".to_string(), "MasterPass123!")
            .unwrap();

        // Verify metadata
        assert_eq!(user.name, "Alice");
        assert!(!user.encrypted_secret_key.is_empty()); // Should be a Base64 string
        assert!(user.vault_ids.is_empty());

        // Verify it now exists on disk
        assert!(manager.is_registered());
        assert!(temp_dir
            .join(format!("{}.json", user.id.to_string()))
            .exists());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_login_fails_with_wrong_password() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        // Setup: Register a user
        let user = manager
            .register("Charlie".to_string(), "RightPassword")
            .unwrap();

        // Action: Attempt to login with the wrong password
        let result = manager.login(&user.id, "WrongPassword");

        // Assertion: Should fail (AES-GCM will fail to decrypt, resulting in an error)
        assert!(result.is_err());
        assert!(matches!(result, Err(VaultError::AuthenticationFailed)));

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_get_user_without_decrypting_key() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        let user = manager
            .register("Dave".to_string(), "SomePassword")
            .unwrap();

        // Action: Call get_user (should not require a password)
        let user = manager.get_user(&user.id).unwrap();

        // Assertion: We get the metadata, but we don't have the raw key
        assert_eq!(user.name, "Dave");
        assert!(!user.encrypted_secret_key.is_empty());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_vault_ownership_tracking() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        let user = manager.register("Eve".to_string(), "Password").unwrap();

        let vault_id_1 = Uuid::new_v4();
        let vault_id_2 = Uuid::new_v4();

        // Add first vault
        manager.add_vault_to_user(vault_id_1).unwrap();
        let user = manager.get_user(&user.id).unwrap();
        assert_eq!(user.vault_ids.len(), 1);
        assert!(user.vault_ids.contains(&vault_id_1));

        // Add second vault
        manager.add_vault_to_user(vault_id_2).unwrap();
        let user = manager.get_user(&user.id).unwrap();
        assert_eq!(user.vault_ids.len(), 2);

        // Remove first vault
        manager.remove_vault_from_user(vault_id_1).unwrap();
        let user = manager.get_user(&user.id).unwrap();
        assert_eq!(user.vault_ids.len(), 1);
        assert!(!user.vault_ids.contains(&vault_id_1));
        assert!(user.vault_ids.contains(&vault_id_2));

        // Adding duplicate shouldn't increase length
        manager.add_vault_to_user(vault_id_2).unwrap();
        let user = manager.get_user(&user.id).unwrap();
        assert_eq!(user.vault_ids.len(), 1);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_migrates_legacy_user_json() {
        let temp_dir = setup();

        // Create a legacy user.json
        let user = User {
            id: Uuid::new_v4(),
            name: "Legacy User".to_string(),
            color: "#6240BF".to_string(),
            icon: None,
            encrypted_secret_key: "legacy-encrypted-key".to_string(),
            vault_ids: Vec::new(),
        };

        let old_path = temp_dir.join("user.json");
        let new_path = temp_dir.join(format!("{}.json", user.id));

        let json = serde_json::to_string_pretty(&user).unwrap();
        std::fs::write(&old_path, json).unwrap();

        // Sanity check: legacy file exists before migration
        assert!(old_path.exists());
        assert!(!new_path.exists());

        // Creating the manager should migrate the file
        let manager = UserManager::new(temp_dir.clone()).unwrap();

        // Legacy file should be gone
        assert!(!old_path.exists());

        // New ID-based file should exist
        assert!(new_path.exists());

        // Verify the migrated file still contains the same user
        let migrated_json = std::fs::read_to_string(&new_path).unwrap();
        let migrated_user: User = serde_json::from_str(&migrated_json).unwrap();

        assert_eq!(migrated_user.id, user.id);
        assert_eq!(migrated_user.name, user.name);
        assert_eq!(
            migrated_user.encrypted_secret_key,
            user.encrypted_secret_key
        );

        // Verify the manager can see the migrated user
        assert!(manager.is_registered());
        assert!(manager.is_user_registered(&user.id));

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
