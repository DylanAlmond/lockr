use argon2::password_hash::rand_core::{OsRng, RngCore};
use std::path::PathBuf;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::{
    crypto::{decrypt, encrypt},
    User, VaultError, VaultId,
};

pub struct UserManager {
    path: PathBuf,
}

impl UserManager {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, VaultError> {
        let path = app_data_dir.join("user.json");
        Ok(Self { path })
    }

    /// Check if a user profile already exists on disk
    pub fn is_registered(&self) -> bool {
        self.path.exists()
    }

    /// Create a brand new user profile.
    /// Generates a random Secret Key and encrypts it with the Master Password.
    pub fn register(&mut self, name: String, master_password: &str) -> Result<User, VaultError> {
        if self.is_registered() {
            return Err(VaultError::Validation("User already registered".into()));
        }

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

        self.save_user(&user)?;

        Ok(user)
    }

    /// Verify the Master Password and decrypt the Secret Key.
    pub fn login(&mut self, master_password: &str) -> Result<Vec<u8>, VaultError> {
        if !self.is_registered() {
            return Err(VaultError::NotFound("User not registered".into()));
        }

        let json = std::fs::read_to_string(&self.path)?;
        let user_file: User = serde_json::from_str(&json)?;

        let decrypted_key = decrypt(&master_password, &user_file.encrypted_secret_key)?;

        // Verify it's exactly 32 bytes (sanity check)
        if decrypted_key.len() != 32 {
            return Err(VaultError::Decryption("Invalid secret key length".into()));
        }

        Ok(decrypted_key)
    }

    /// Get the user profile metadata (does NOT decrypt the secret key)
    pub fn get_user(&self) -> Result<User, VaultError> {
        if !self.is_registered() {
            return Err(VaultError::NotFound("User not registered".into()));
        }
        let json = std::fs::read_to_string(&self.path)?;
        let user_file: User = serde_json::from_str(&json)?;

        Ok(user_file)
    }

    /// Update profile details (name, color, icon)
    pub fn update_profile(
        &mut self,
        name: Option<String>,
        color: Option<String>,
        icon: Option<String>,
    ) -> Result<(), VaultError> {
        let mut user = self.get_user()?;

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

    /// Add a newly created vault to the user's list
    pub fn add_vault_to_user(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let mut user = self.get_user()?;
        if !user.vault_ids.contains(&vault_id) {
            user.vault_ids.push(vault_id);
            self.save_user(&user)?;
        }
        Ok(())
    }

    /// Remove a vault from the user's list (e.g., on deletion)
    pub fn remove_vault_from_user(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let mut user = self.get_user()?;
        user.vault_ids.retain(|id| id != &vault_id);
        self.save_user(&user)?;
        Ok(())
    }

    // Delete the current user
    pub fn delete_user(&self) -> Result<(), VaultError> {
        std::fs::remove_file(&self.path).map_err(VaultError::Io)?;
        Ok(())
    }

    fn save_user(&self, user: &User) -> Result<(), VaultError> {
        let json = serde_json::to_string_pretty(&user)?;
        std::fs::write(&self.path, json)?;
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
        assert!(temp_dir.join("user.json").exists());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_login_fails_with_wrong_password() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        // Setup: Register a user
        manager
            .register("Charlie".to_string(), "RightPassword")
            .unwrap();

        // Action: Attempt to login with the wrong password
        let result = manager.login("WrongPassword");

        // Assertion: Should fail (AES-GCM will fail to decrypt, resulting in an error)
        assert!(result.is_err());
        assert!(matches!(result, Err(VaultError::AuthenticationFailed)));

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_get_user_without_decrypting_key() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        manager
            .register("Dave".to_string(), "SomePassword")
            .unwrap();

        // Action: Call get_user (should not require a password)
        let user = manager.get_user().unwrap();

        // Assertion: We get the metadata, but we don't have the raw key
        assert_eq!(user.name, "Dave");
        assert!(!user.encrypted_secret_key.is_empty());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_vault_ownership_tracking() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        manager.register("Eve".to_string(), "Password").unwrap();

        let vault_id_1 = Uuid::new_v4();
        let vault_id_2 = Uuid::new_v4();

        // Add first vault
        manager.add_vault_to_user(vault_id_1).unwrap();
        let user = manager.get_user().unwrap();
        assert_eq!(user.vault_ids.len(), 1);
        assert!(user.vault_ids.contains(&vault_id_1));

        // Add second vault
        manager.add_vault_to_user(vault_id_2).unwrap();
        let user = manager.get_user().unwrap();
        assert_eq!(user.vault_ids.len(), 2);

        // Remove first vault
        manager.remove_vault_from_user(vault_id_1).unwrap();
        let user = manager.get_user().unwrap();
        assert_eq!(user.vault_ids.len(), 1);
        assert!(!user.vault_ids.contains(&vault_id_1));
        assert!(user.vault_ids.contains(&vault_id_2));

        // Adding duplicate shouldn't increase length
        manager.add_vault_to_user(vault_id_2).unwrap();
        let user = manager.get_user().unwrap();
        assert_eq!(user.vault_ids.len(), 1);

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
