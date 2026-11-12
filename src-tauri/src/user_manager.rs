use argon2::password_hash::rand_core::{OsRng, RngCore};
use chrono::Utc;
use std::path::PathBuf;
use uuid::Uuid;
use zeroize::Zeroize;

use crate::atomic::write_atomic;
use crate::crypto::{decrypt, encrypt};
use crate::device::{device_name, device_platform, load_or_create_device_id, DeviceKeypair};
use crate::registry::{DeviceRecord, DeviceRegistry, VaultRegistry};
use crate::{DeviceId, DeviceProfile, SharedProfile, User, UserId, VaultError, VaultId};

/// Suffix of the shared half of a profile, beside the device-local half.
const SHARED_PROFILE_SUFFIX: &str = ".shared.json";

pub struct UserManager {
    app_data_dir: PathBuf,

    /// This installation's Device identity, shared by every User profile on
    /// this Device.
    device_id: DeviceId,

    pub active_user_id: Option<UserId>,
}

impl UserManager {
    pub fn new(app_data_dir: PathBuf) -> Result<Self, VaultError> {
        let device_id = load_or_create_device_id(&app_data_dir)?;

        let manager = Self {
            app_data_dir,
            device_id,
            active_user_id: None,
        };

        manager.migrate_legacy_profile()?;

        Ok(manager)
    }

    /// This installation's Device identity.
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    // ============ Paths ============

    fn local_path(&self, id: &UserId) -> PathBuf {
        self.app_data_dir.join(format!("{id}.json"))
    }

    fn shared_path(&self, id: &UserId) -> PathBuf {
        self.app_data_dir
            .join(format!("{id}{SHARED_PROFILE_SUFFIX}"))
    }

    // ============ Migration from the single-file profile ============

    /// Split a pre-split `user.json` into a device-local profile and a shared
    /// profile. The legacy file is renamed into the device-local slot, so its
    /// pre-split fields survive for a one-way migration only.
    ///
    /// `vault_ids` are copied into the shared Vault Registry *here*, at the
    /// migration boundary. The fallback in [`Self::load_shared_profile_or_legacy`]
    /// deliberately does not read them again: after this point the shared
    /// registry is the only authority, so a lost shared file cannot resurrect
    /// a stale vault list.
    fn migrate_legacy_profile(&self) -> Result<(), VaultError> {
        let old_path = self.app_data_dir.join("user.json");
        if !old_path.exists() {
            return Ok(());
        }

        let json = std::fs::read_to_string(&old_path)?;
        let user: User = serde_json::from_str(&json)?;

        let new_path = self.local_path(&user.id);

        // Don't overwrite an existing modern file.
        if !new_path.exists() {
            std::fs::rename(&old_path, &new_path)?;
        }

        let shared_path = self.shared_path(&user.id);
        if !shared_path.exists() {
            let mut vaults = VaultRegistry::new();
            for vault_id in &user.vault_ids {
                vaults.insert(*vault_id);
            }

            let shared = SharedProfile {
                user_id: user.id,
                name: user.name,
                color: user.color,
                icon: user.icon,
                devices: DeviceRegistry::new(),
                vaults,
            };
            self.save_shared_profile(&shared)?;
        }

        Ok(())
    }

    // ============ Queries ============

    // Get all valid accounts listed in app data dir
    pub fn get_users(&self) -> Result<Vec<User>, VaultError> {
        let mut available_accounts: Vec<User> = Vec::new();

        for entry in std::fs::read_dir(&self.app_data_dir)? {
            let path = entry?.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if file_name.ends_with(SHARED_PROFILE_SUFFIX) {
                continue;
            }

            let json = std::fs::read_to_string(&path)?;

            match serde_json::from_str::<DeviceProfile>(&json) {
                Ok(profile) => match self.load_shared_profile_or_legacy(&profile) {
                    Ok(shared) => available_accounts.push(user_view(&profile, &shared)),
                    Err(err) => eprintln!(
                        "Failed to verify shared profile for user {}: {}",
                        profile.id, err
                    ),
                },
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
        self.local_path(id).exists()
    }

    // ============ Registration and login ============

    /// Create a brand new user profile.
    /// Generates a random Secret Key and encrypts it with the Master Password.
    pub fn register(&mut self, name: String, master_password: &str) -> Result<User, VaultError> {
        let mut secret_key_bytes = [0u8; 32];
        OsRng.fill_bytes(&mut secret_key_bytes);

        let encrypted_secret_key = encrypt(master_password, &secret_key_bytes)?;

        // Wipe the raw key from this local scope immediately
        secret_key_bytes.zeroize();

        let keypair = DeviceKeypair::generate();
        let encrypted_signing_key = keypair.wrap(master_password)?;
        let signing_public_key = keypair.public_key();

        let user_id = Uuid::new_v4();

        // The first Device admits itself. Trust on first use, for the root
        // record only; every later admission is signed by an admitted Device.
        let self_record = DeviceRecord::admit(
            self.device_id,
            signing_public_key.clone(),
            device_name(),
            device_platform(),
            self.device_id,
            Utc::now(),
            &keypair,
        );

        let mut devices = DeviceRegistry::new();
        devices.admit(self_record)?;

        let shared = SharedProfile {
            user_id,
            name,
            color: "#6240BF".to_string(),
            icon: None,
            devices,
            vaults: VaultRegistry::new(),
        };

        let local = DeviceProfile {
            id: user_id,
            encrypted_secret_key,
            device_id: Some(self.device_id),
            encrypted_signing_key,
            signing_public_key,
            name: String::new(),
            color: String::new(),
            icon: None,
            vault_ids: Vec::new(),
        };

        self.active_user_id = Some(user_id);
        self.save_local_profile(&local)?;
        self.save_shared_profile(&shared)?;

        Ok(user_view(&local, &shared))
    }

    /// Verify the Master Password and decrypt the Secret Key.
    pub fn login(&mut self, id: &UserId, master_password: &str) -> Result<Vec<u8>, VaultError> {
        if !self.is_user_registered(id) {
            return Err(VaultError::UserNotFound("User not registered".into()));
        }

        let mut local = self.load_local_profile(id)?;

        let mut decrypted_key = decrypt(master_password, &local.encrypted_secret_key)?;

        // Verify it's exactly 32 bytes (sanity check)
        if decrypted_key.len() != 32 {
            decrypted_key.zeroize();
            return Err(VaultError::Decryption("Invalid secret key length".into()));
        }

        // A profile written before the split has no Device signing key. Mint
        // one now and bind this profile to this installation.
        let mut local_changed = false;
        let keypair =
            self.ensure_device_keypair(&mut local, master_password, &mut local_changed)?;

        let mut shared = self.load_or_create_shared_profile(&local)?;
        let mut shared_changed = false;
        self.ensure_self_admission(&mut shared, &keypair, &mut shared_changed)?;

        if local_changed {
            self.save_local_profile(&local)?;
        }
        if shared_changed {
            self.save_shared_profile(&shared)?;
        }

        self.active_user_id = Some(local.id);

        Ok(decrypted_key)
    }

    /// Make sure this profile has a Device keypair whose private half is
    /// wrapped under the master password, and that it belongs to this
    /// installation.
    fn ensure_device_keypair(
        &self,
        local: &mut DeviceProfile,
        master_password: &str,
        changed: &mut bool,
    ) -> Result<DeviceKeypair, VaultError> {
        let bound_to_this_device = local.device_id == Some(self.device_id);
        let has_wrapped_key = !local.encrypted_signing_key.is_empty();

        if bound_to_this_device && has_wrapped_key {
            let keypair =
                DeviceKeypair::from_wrapped(master_password, &local.encrypted_signing_key)?;
            if local.signing_public_key != keypair.public_key() {
                local.signing_public_key = keypair.public_key();
                *changed = true;
            }
            return Ok(keypair);
        }

        let keypair = DeviceKeypair::generate();
        local.device_id = Some(self.device_id);
        local.encrypted_signing_key = keypair.wrap(master_password)?;
        local.signing_public_key = keypair.public_key();
        *changed = true;
        Ok(keypair)
    }

    /// A registry without this Device's admission cannot make it sign
    /// anything other Devices will accept. On an empty (pre-split) registry
    /// this Device is the root and admits itself; otherwise another admitted
    /// Device must have admitted it during pairing. The registry refuses a
    /// self-admission once a root exists, so this is not a way in.
    /// It also refuses to silently replace a different key recorded for this
    /// Device id.
    fn ensure_self_admission(
        &self,
        shared: &mut SharedProfile,
        keypair: &DeviceKeypair,
        changed: &mut bool,
    ) -> Result<(), VaultError> {
        if let Some(existing) = shared.devices.get(&self.device_id) {
            if existing.public_key != keypair.public_key() {
                return Err(VaultError::DeviceIdentityDamaged(format!(
                    "Device {} is already admitted with a different key. Restore this \
                     Device's signing key from backup, or start with a new identity.",
                    self.device_id
                )));
            }
            return Ok(());
        }

        let record = DeviceRecord::admit(
            self.device_id,
            keypair.public_key(),
            device_name(),
            device_platform(),
            self.device_id,
            Utc::now(),
            keypair,
        );
        shared.devices.admit(record)?;
        *changed = true;
        Ok(())
    }

    // Logout the active user
    pub fn logout(&mut self) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

        if !self.is_user_registered(user_id) {
            return Err(VaultError::UserNotFound("User not registered".into()));
        }

        self.active_user_id = None;

        Ok(())
    }

    // ============ Device-local profile ============

    fn load_local_profile(&self, id: &UserId) -> Result<DeviceProfile, VaultError> {
        let path = self.local_path(id);
        if !path.exists() {
            return Err(VaultError::UserNotFound("User not registered".into()));
        }

        let json = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&json)?)
    }

    fn save_local_profile(&self, profile: &DeviceProfile) -> Result<(), VaultError> {
        let json = serde_json::to_string_pretty(profile)?;
        write_atomic(&self.local_path(&profile.id), json.as_bytes())?;
        Ok(())
    }

    // ============ Shared profile ============

    /// The shared half of the active User's profile. Useful to callers that
    /// need the registries without assembling the UI-facing [`User`].
    pub fn get_shared_profile(&self, id: &UserId) -> Result<SharedProfile, VaultError> {
        let local = self.load_local_profile(id)?;
        self.load_shared_profile_or_legacy(&local)
    }

    pub fn save_shared_profile(&self, profile: &SharedProfile) -> Result<(), VaultError> {
        let json = serde_json::to_string_pretty(profile)?;
        write_atomic(&self.shared_path(&profile.user_id), json.as_bytes())?;
        Ok(())
    }

    fn load_shared_profile(&self, id: &UserId) -> Result<SharedProfile, VaultError> {
        let path = self.shared_path(id);
        if !path.exists() {
            return Err(VaultError::NotFound(format!(
                "Shared profile for User {id} is missing"
            )));
        }

        let json = std::fs::read_to_string(&path)?;
        let shared: SharedProfile = serde_json::from_str(&json)?;

        // The shared file is untrusted input: a tampered admission or
        // revocation must be rejected here, not trusted until the next
        // registry operation reads it.
        shared.devices.verify()?;

        Ok(shared)
    }

    /// Read the shared half, falling back to the fields a pre-split profile
    /// carried so migration never needs the master password.
    ///
    /// Only a *missing* shared file falls back. An unreadable or invalid one
    /// is an error: silently substituting legacy fields would trust a file
    /// that failed verification.
    fn load_shared_profile_or_legacy(
        &self,
        local: &DeviceProfile,
    ) -> Result<SharedProfile, VaultError> {
        if self.shared_path(&local.id).exists() {
            return self.load_shared_profile(&local.id);
        }

        Ok(SharedProfile {
            user_id: local.id,
            name: local.name.clone(),
            color: local.color.clone(),
            icon: local.icon.clone(),
            devices: DeviceRegistry::new(),
            // Legacy `vault_ids` are deliberately ignored: after the split
            // the shared Vault Registry is authoritative, and a missing
            // shared file must not resurrect a stale membership list.
            vaults: VaultRegistry::new(),
        })
    }

    fn load_or_create_shared_profile(
        &self,
        local: &DeviceProfile,
    ) -> Result<SharedProfile, VaultError> {
        if self.shared_path(&local.id).exists() {
            return self.load_shared_profile(&local.id);
        }

        let shared = self.load_shared_profile_or_legacy(local)?;
        self.save_shared_profile(&shared)?;
        Ok(shared)
    }

    // ============ UI-facing profile ============

    /// Get the user profile metadata (does NOT decrypt the secret key)
    pub fn get_user(&self, id: &UserId) -> Result<User, VaultError> {
        let local = self.load_local_profile(id)?;
        let shared = self.load_shared_profile_or_legacy(&local)?;
        Ok(user_view(&local, &shared))
    }

    /// Update the active user's shared profile details (name, color, icon).
    /// The device-local half is untouched.
    pub fn update_profile(
        &mut self,
        name: Option<String>,
        color: Option<String>,
        icon: Option<String>,
    ) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

        let mut shared = self.get_shared_profile(user_id)?;

        if let Some(n) = name {
            shared.name = n;
        }
        if let Some(c) = color {
            shared.color = c;
        }
        if let Some(i) = icon {
            shared.icon = if i.is_empty() { None } else { Some(i) };
        }

        self.save_shared_profile(&shared)
    }

    // ============ Vault Registry views ============

    /// Add a newly created vault to the active user's Vault Registry.
    pub fn add_vault_to_user(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

        let mut shared = self.get_shared_profile(user_id)?;
        if shared.vaults.insert(vault_id) {
            self.save_shared_profile(&shared)?;
        }
        Ok(())
    }

    /// Remove a vault from the user's Vault Registry (e.g., on deletion).
    pub fn remove_vault_from_user(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

        let mut shared = self.get_shared_profile(user_id)?;
        if shared.vaults.remove(&vault_id) {
            self.save_shared_profile(&shared)?;
        }
        Ok(())
    }

    // Delete the current user
    pub fn delete_user(&mut self) -> Result<(), VaultError> {
        if self.active_user_id.is_none() {
            return Err(VaultError::UserNotFound("No user is active".into()));
        }

        let user_id = self
            .active_user_id
            .as_ref()
            .expect("checked above")
            .to_owned();

        let local_path = self.local_path(&user_id);
        if local_path.exists() {
            std::fs::remove_file(local_path).map_err(VaultError::Io)?;
        }

        let shared_path = self.shared_path(&user_id);
        if shared_path.exists() {
            std::fs::remove_file(shared_path).map_err(VaultError::Io)?;
        }

        self.active_user_id = None;

        Ok(())
    }
}

/// Assemble the UI-facing profile from its two halves. `vault_ids` is a view
/// over the shared Vault Registry, never a stored list.
fn user_view(local: &DeviceProfile, shared: &SharedProfile) -> User {
    User {
        id: local.id,
        name: shared.name.clone(),
        color: shared.color.clone(),
        icon: shared.icon.clone(),
        encrypted_secret_key: local.encrypted_secret_key.clone(),
        vault_ids: shared.vault_ids(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::encrypt;
    use crate::User;
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

    fn shared_path(temp_dir: &PathBuf, user_id: &UserId) -> PathBuf {
        temp_dir.join(format!("{user_id}.shared.json"))
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

        // The shared half exists beside it.
        assert!(shared_path(&temp_dir, &user.id).exists());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_profile_split_keeps_the_secret_key_local_and_metadata_shared() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let user = manager
            .register("Split User".to_string(), "MasterPass123!")
            .unwrap();

        let local_json =
            std::fs::read_to_string(temp_dir.join(format!("{}.json", user.id))).unwrap();
        let local: serde_json::Value = serde_json::from_str(&local_json).unwrap();

        assert!(local["encrypted_secret_key"].is_string());
        assert!(local["encrypted_signing_key"].is_string());
        assert_eq!(
            local["device_id"],
            manager.device_id().to_string(),
            "the local profile is bound to this installation"
        );
        assert!(local.get("name").is_none(), "name is shared state");
        assert!(local.get("color").is_none(), "color is shared state");
        assert!(local.get("icon").is_none(), "icon is shared state");
        assert!(
            local.get("vault_ids").is_none(),
            "vault ids are a view over the Vault Registry"
        );

        let shared_json = std::fs::read_to_string(shared_path(&temp_dir, &user.id)).unwrap();
        let shared: serde_json::Value = serde_json::from_str(&shared_json).unwrap();
        assert_eq!(shared["name"], "Split User");
        assert_eq!(shared["color"], "#6240BF");
        assert!(shared["devices"]["devices"].is_object());
        assert!(
            shared.get("encrypted_secret_key").is_none(),
            "the wrapped secret key must stay device-local"
        );
        assert!(
            shared.get("encrypted_signing_key").is_none(),
            "the wrapped Device signing key must stay device-local"
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_a_missing_shared_profile_does_not_resurrect_legacy_vault_ids() {
        let temp_dir = setup();
        let manager = UserManager::new(temp_dir.clone()).unwrap();
        let user_id = Uuid::new_v4();
        let stale_vault = Uuid::new_v4();

        // A pre-split local profile still carries the old `vault_ids` list,
        // but after the split only the shared Vault Registry is authoritative.
        let local = serde_json::json!({
            "id": user_id,
            "encrypted_secret_key": "legacy-key",
            "device_id": manager.device_id(),
            "encrypted_signing_key": "legacy-signing-key",
            "signing_public_key": "legacy-public-key",
            "name": "Old Name",
            "color": "#111111",
            "icon": null,
            "vault_ids": [stale_vault],
        });
        std::fs::write(
            temp_dir.join(format!("{user_id}.json")),
            serde_json::to_string_pretty(&local).unwrap(),
        )
        .unwrap();

        let shared = manager.get_shared_profile(&user_id).unwrap();

        assert!(
            shared.vault_ids().is_empty(),
            "a missing shared file must not resurrect the legacy vault list"
        );
        assert!(
            manager.get_user(&user_id).unwrap().vault_ids.is_empty(),
            "the UI-facing view must not gain a stale vault"
        );
        assert_eq!(shared.name, "Old Name", "metadata fallback still works");

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_vault_ids_are_derived_from_the_vault_registry() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let user = manager
            .register("View User".to_string(), "Password")
            .unwrap();
        let vault_id = Uuid::new_v4();

        manager.add_vault_to_user(vault_id).unwrap();

        let shared = manager.get_shared_profile(&user.id).unwrap();
        assert!(shared.vaults.contains(&vault_id));
        assert_eq!(
            manager.get_user(&user.id).unwrap().vault_ids,
            vec![vault_id]
        );

        // Removing from the registry is immediately reflected in the view.
        manager.remove_vault_from_user(vault_id).unwrap();
        assert!(manager.get_user(&user.id).unwrap().vault_ids.is_empty());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_updating_shared_metadata_does_not_touch_the_local_profile() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let user = manager.register("Before".to_string(), "Password").unwrap();

        let local_path = temp_dir.join(format!("{}.json", user.id));
        let local_before = std::fs::read(&local_path).unwrap();

        manager
            .update_profile(Some("After".to_string()), None, None)
            .unwrap();

        assert_eq!(
            std::fs::read(&local_path).unwrap(),
            local_before,
            "shared metadata must not rewrite the wrapped secret key"
        );
        assert_eq!(manager.get_user(&user.id).unwrap().name, "After");

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_login_mints_a_device_keypair_for_a_pre_split_profile() {
        let temp_dir = setup();
        let user_id = Uuid::new_v4();
        let secret_key = [7u8; 32];
        let legacy = User {
            id: user_id,
            name: "Legacy User".to_string(),
            color: "#6240BF".to_string(),
            icon: None,
            encrypted_secret_key: encrypt("Password", &secret_key).unwrap(),
            vault_ids: vec![Uuid::new_v4()],
        };
        std::fs::write(
            temp_dir.join("user.json"),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let unlocked = manager.login(&user_id, "Password").unwrap();

        assert_eq!(unlocked, secret_key);

        let local: DeviceProfile = serde_json::from_str(
            &std::fs::read_to_string(temp_dir.join(format!("{user_id}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(local.device_id, Some(manager.device_id()));
        assert!(!local.encrypted_signing_key.is_empty());

        let shared = manager.get_shared_profile(&user_id).unwrap();
        let record = shared.devices.get(&manager.device_id()).unwrap();
        assert_eq!(record.public_key, local.signing_public_key);
        assert!(!record.signature.is_empty());
        shared.devices.verify().unwrap();
        assert_eq!(shared.vault_ids(), legacy.vault_ids);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_a_tampered_shared_profile_fails_verification_instead_of_being_trusted() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let user = manager
            .register("Tamper Target".to_string(), "Password")
            .unwrap();

        let path = shared_path(&temp_dir, &user.id);
        let mut shared: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let devices = shared["devices"]["devices"].as_object_mut().unwrap();
        let record = devices
            .get_mut(&manager.device_id().to_string())
            .expect("the root Device is in the registry");
        record["name"] = serde_json::json!("Attacker's Laptop");
        std::fs::write(&path, serde_json::to_string_pretty(&shared).unwrap()).unwrap();

        let error = manager
            .get_shared_profile(&user.id)
            .expect_err("a tampered shared profile must not be trusted");
        assert!(matches!(error, VaultError::DeviceRegistry(_)));
        assert!(
            manager.get_user(&user.id).is_err(),
            "the UI-facing profile must not be assembled from an unverified registry"
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_shared_profile_registry_can_be_read_and_saved_for_sync() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let user = manager
            .register("Sync User".to_string(), "Password")
            .unwrap();

        let mut shared = manager.get_shared_profile(&user.id).unwrap();
        shared.name = "Synced Name".to_string();
        manager.save_shared_profile(&shared).unwrap();

        let reread = manager.get_shared_profile(&user.id).unwrap();
        assert_eq!(reread.name, "Synced Name");
        assert!(reread.devices.get(&manager.device_id()).is_some());

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
