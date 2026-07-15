use std::path::PathBuf;
use uuid::Uuid;

use crate::{User, VaultError, VaultId};

pub struct UserManager {
    path: PathBuf,
    user: User,
}

impl UserManager {
    /// Load user from disk, or create a default one if it doesn't exist
    pub fn new(app_data_dir: PathBuf) -> Result<Self, VaultError> {
        let path = app_data_dir.join("user.json");

        let user = if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            let user_file: User = serde_json::from_str(&json)?;

            User {
                id: user_file.id,
                name: user_file.name,
                color: user_file.color,
                icon: user_file.icon,
                active_vault_id: user_file.active_vault_id,
            }
        } else {
            let default_user = User {
                id: Uuid::new_v4(),
                name: "Default User".to_string(),
                color: "#6240BF".to_string(),
                icon: "user".to_string(),
                active_vault_id: None,
            };

            let json = serde_json::to_string_pretty(&User {
                id: default_user.id,
                name: default_user.name.clone(),
                color: default_user.color.clone(),
                icon: default_user.icon.clone(),
                active_vault_id: default_user.active_vault_id,
            })?;

            std::fs::write(&path, json)?;

            default_user
        };

        Ok(Self { path, user })
    }

    /// Get a reference to the current user
    pub fn get_user(&self) -> &User {
        &self.user
    }

    /// Update profile details (name, color, icon)
    pub fn update_profile(
        &mut self,
        name: Option<String>,
        color: Option<String>,
        icon: Option<String>,
    ) -> Result<(), VaultError> {
        if let Some(n) = name {
            self.user.name = n;
        }
        if let Some(c) = color {
            self.user.color = c;
        }
        if let Some(i) = icon {
            self.user.icon = i;
        }

        self.save()
    }

    /// Set which vault was last open
    pub fn set_active_vault(&mut self, vault_id: Option<VaultId>) -> Result<(), VaultError> {
        self.user.active_vault_id = vault_id;
        self.save()
    }

    /// Internal save function
    fn save(&self) -> Result<(), VaultError> {
        let user_file = User {
            id: self.user.id,
            name: self.user.name.clone(),
            color: self.user.color.clone(),
            icon: self.user.icon.clone(),
            active_vault_id: self.user.active_vault_id,
        };

        let json = serde_json::to_string_pretty(&user_file)?;
        std::fs::write(&self.path, json)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> PathBuf {
        let temp_dir = std::env::temp_dir().join("user_manager_test");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_creates_default_user_on_first_run() {
        let temp_dir = setup();
        let manager = UserManager::new(temp_dir.clone()).unwrap();

        let user = manager.get_user();
        assert_eq!(user.name, "Default User");
        assert_eq!(user.color, "#6240BF");
        assert!(user.active_vault_id.is_none());

        // Verify file was actually created
        assert!(temp_dir.join("user.json").exists());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_loads_existing_user() {
        let temp_dir = setup();

        // First run creates it
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let original_id = manager.get_user().id;

        // Modify and save
        manager
            .update_profile(Some("Alice".to_string()), None, None)
            .unwrap();

        // Second run should load the modified data, NOT create a new user
        let manager2 = UserManager::new(temp_dir.clone()).unwrap();
        let user2 = manager2.get_user();

        assert_eq!(user2.id, original_id); // Same ID!
        assert_eq!(user2.name, "Alice"); // Updated name!

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_set_active_vault() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();

        let fake_vault_id = Uuid::new_v4();
        manager.set_active_vault(Some(fake_vault_id)).unwrap();

        assert_eq!(manager.get_user().active_vault_id, Some(fake_vault_id));

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
