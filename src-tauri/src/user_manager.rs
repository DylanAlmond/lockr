use argon2::password_hash::rand_core::{OsRng, RngCore};
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    println,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::Zeroize;
use zxcvbn::zxcvbn;

use crate::{
    crypto::{decrypt, encrypt},
    icons::{safe_icon, validate_icon},
    User, UserId, VaultError, VaultId,
};

/// Master-password policy and login throttle (docs/security/auth-policy.md).
pub const MASTER_PASSWORD_MIN_LEN: usize = 12;
pub const MASTER_PASSWORD_MIN_SCORE: u8 = 3;
const MASTER_PASSWORD_CONTEXT: &[&str] = &["lockr", "vault", "password"];
const LOGIN_MAX_FAILURES: u32 = 5;
const LOGIN_BACKOFF_BASE: Duration = Duration::from_secs(30);
const LOGIN_BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);

struct LoginThrottle {
    failures: u32,
    blocked_until: Option<Instant>,
}

pub struct UserManager {
    app_data_dir: PathBuf,
    pub active_user_id: Option<UserId>,
    active_path: Option<PathBuf>,

    /// Consecutive failed logins per user. In memory only: a restart clears it.
    throttle: HashMap<UserId, LoginThrottle>,
}

/// Reject a master password that does not meet the documented policy. Context
/// words include the user's name and the product, so predictable passwords
/// built from them score low.
pub fn validate_master_password(name: &str, master_password: &str) -> Result<(), VaultError> {
    if master_password.chars().count() < MASTER_PASSWORD_MIN_LEN {
        return Err(VaultError::Validation(format!(
            "Master password must be at least {MASTER_PASSWORD_MIN_LEN} characters"
        )));
    }

    let mut inputs: Vec<&str> = MASTER_PASSWORD_CONTEXT.to_vec();
    inputs.extend(name.split_whitespace());

    let entropy = zxcvbn(master_password, &inputs);
    let score = u8::from(entropy.score());
    if score < MASTER_PASSWORD_MIN_SCORE {
        return Err(VaultError::Validation(format!(
            "Master password is too weak (score {score}/4); use a longer, less predictable password"
        )));
    }

    Ok(())
}

/// Blocking delay after `failures` consecutive failures, capped. `None` means
/// the next attempt is still allowed immediately.
fn login_backoff(failures: u32) -> Option<Duration> {
    if failures < LOGIN_MAX_FAILURES {
        return None;
    }

    let exponent = (failures - LOGIN_MAX_FAILURES).min(10);
    Some(
        LOGIN_BACKOFF_BASE
            .saturating_mul(1u32 << exponent)
            .min(LOGIN_BACKOFF_MAX),
    )
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
            throttle: HashMap::new(),
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
                Ok(mut user_data) => {
                    user_data.icon = safe_icon(user_data.icon.as_deref());
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

    /// Append one authentication event to `auth.log`. Best-effort and never
    /// contains secret material.
    fn audit(&self, event: &str, user: Option<&UserId>) {
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let line = match user {
            Some(id) => format!("{timestamp} {event} user={id}\n"),
            None => format!("{timestamp} {event}\n"),
        };

        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.app_data_dir.join("auth.log"))
            .and_then(|mut file| file.write_all(line.as_bytes()));
    }

    fn check_login_allowed(&self, id: &UserId) -> Result<(), VaultError> {
        let Some(throttle) = self.throttle.get(id) else {
            return Ok(());
        };
        let Some(blocked_until) = throttle.blocked_until else {
            return Ok(());
        };

        let now = Instant::now();
        if blocked_until > now {
            let remaining = blocked_until
                .saturating_duration_since(now)
                .as_secs()
                .max(1);
            return Err(VaultError::TooManyAttempts(remaining));
        }

        Ok(())
    }

    fn record_failed_login(&mut self, id: &UserId) {
        let throttle = self.throttle.entry(*id).or_insert(LoginThrottle {
            failures: 0,
            blocked_until: None,
        });
        throttle.failures = throttle.failures.saturating_add(1);

        if let Some(delay) = login_backoff(throttle.failures) {
            throttle.blocked_until = Some(Instant::now() + delay);
        }
    }

    fn record_successful_login(&mut self, id: &UserId) {
        self.throttle.remove(id);
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
        self.audit("registered", Some(&user.id));

        Ok(user)
    }

    /// Verify the Master Password and decrypt the Secret Key.
    pub fn login(&mut self, id: &UserId, master_password: &str) -> Result<Vec<u8>, VaultError> {
        if let Err(error) = self.check_login_allowed(id) {
            self.audit("login_blocked", Some(id));
            return Err(error);
        }

        if !self.is_user_registered(id) {
            return Err(VaultError::UserNotFound("User not registered".into()));
        }

        let path = self.app_data_dir.join(format!("{}.json", id.to_string()));

        let json: String = std::fs::read_to_string(&path)?;

        let user_file: User = serde_json::from_str(&json)?;

        let decrypted_key =
            match Self::decrypt_secret_key(master_password, &user_file.encrypted_secret_key) {
                Ok(key) => key,
                Err(error) => {
                    self.record_failed_login(id);
                    self.audit("login_failed", Some(id));
                    return Err(error);
                }
            };

        self.record_successful_login(id);
        self.audit("login_success", Some(id));

        self.active_user_id = Some(user_file.id);
        self.active_path = Some(path);

        Ok(decrypted_key)
    }

    /// Forget the active user. Session teardown is the caller's job; this only
    /// clears which profile is current, even if its file has since vanished.
    pub fn clear_active_user(&mut self) {
        self.active_user_id = None;
        self.active_path = None;
    }

    /// The profile of the user that is currently logged in.
    pub fn active_user(&self) -> Result<User, VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

        self.get_user(user_id)
    }

    /// Re-check the master password of the active user without changing the
    /// session. The decrypted secret key is zeroized before returning, so a
    /// successful check leaves no new copy of key material behind.
    pub fn verify_password(&mut self, master_password: &str) -> Result<(), VaultError> {
        let user = self.active_user()?;

        if let Err(error) = self.check_login_allowed(&user.id) {
            self.audit("reauth_blocked", Some(&user.id));
            return Err(error);
        }

        let mut secret_key =
            match Self::decrypt_secret_key(master_password, &user.encrypted_secret_key) {
                Ok(key) => key,
                Err(error) => {
                    self.record_failed_login(&user.id);
                    self.audit("reauth_failed", Some(&user.id));
                    return Err(error);
                }
            };
        secret_key.zeroize();
        self.record_successful_login(&user.id);

        Ok(())
    }

    /// Whether the active user's profile lists this vault. The profile's
    /// `vault_ids` is the access-control list vault-scoped commands check.
    pub fn owns_vault(&self, vault_id: VaultId) -> Result<bool, VaultError> {
        Ok(self.active_user()?.vault_ids.contains(&vault_id))
    }

    /// Decrypt a user's secret key with a master password and sanity-check
    /// its length. Callers that only verify the password must zeroize the
    /// returned key.
    fn decrypt_secret_key(
        master_password: &str,
        encrypted_secret_key: &str,
    ) -> Result<Vec<u8>, VaultError> {
        let secret_key = decrypt(master_password, encrypted_secret_key)?;

        if secret_key.len() != 32 {
            return Err(VaultError::Decryption("Invalid secret key length".into()));
        }

        Ok(secret_key)
    }

    /// Get the user profile metadata (does NOT decrypt the secret key)
    pub fn get_user(&self, id: &UserId) -> Result<User, VaultError> {
        if !self.is_user_registered(id) {
            return Err(VaultError::UserNotFound("User not registered".into()));
        }

        let path = self
            .active_path
            .as_ref()
            .ok_or(VaultError::UserNotFound("active_path is not set".into()))?;

        let json: String = std::fs::read_to_string(path)?;
        let mut user_file: User = serde_json::from_str(&json)?;
        user_file.icon = safe_icon(user_file.icon.as_deref());

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
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

        let mut user = self.get_user(user_id)?;

        if let Some(n) = name {
            user.name = n;
        }
        if let Some(c) = color {
            user.color = c;
        }
        if let Some(i) = icon {
            user.icon = validate_icon(Some(i))?;
        }

        self.save_user(&user)
    }

    /// Add a newly created vault to the active user's list
    pub fn add_vault_to_user(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let user_id = self
            .active_user_id
            .as_ref()
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

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
            .ok_or_else(|| VaultError::UserNotFound("No user is active".into()))?;

        let mut user = self.get_user(user_id)?;
        user.vault_ids.retain(|id| id != &vault_id);
        self.save_user(&user)?;
        Ok(())
    }

    // Delete the current user
    pub fn delete_user(&mut self) -> Result<(), VaultError> {
        if self.active_user_id.is_none() {
            return Err(VaultError::UserNotFound("No user is active".into()));
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
    fn test_master_password_policy() {
        assert!(validate_master_password("Alice", "short").is_err());
        assert!(
            validate_master_password("Alice", "passwordpassword").is_err(),
            "a predictable password must not pass the score floor"
        );
        assert!(validate_master_password("Alice", "correct horse battery staple").is_ok());
    }

    #[test]
    fn test_login_backoff_grows_and_caps() {
        assert_eq!(login_backoff(4), None);
        assert_eq!(login_backoff(5), Some(Duration::from_secs(30)));
        assert_eq!(login_backoff(6), Some(Duration::from_secs(60)));
        assert_eq!(login_backoff(20), Some(LOGIN_BACKOFF_MAX));
    }

    #[test]
    fn test_login_throttle_blocks_and_clears() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let id = Uuid::new_v4();

        for _ in 0..4 {
            manager.record_failed_login(&id);
        }
        manager.check_login_allowed(&id).unwrap();

        manager.record_failed_login(&id);
        assert!(matches!(
            manager.check_login_allowed(&id),
            Err(VaultError::TooManyAttempts(_))
        ));

        manager.record_successful_login(&id);
        manager.check_login_allowed(&id).unwrap();

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_audit_log_records_events_without_secrets() {
        let temp_dir = setup();
        let manager = UserManager::new(temp_dir.clone()).unwrap();
        let id = Uuid::new_v4();

        manager.audit("login_failed", Some(&id));

        let log = std::fs::read_to_string(temp_dir.join("auth.log")).unwrap();
        assert!(log.contains("login_failed"));
        assert!(log.contains(&id.to_string()));

        std::fs::remove_dir_all(temp_dir).ok();
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
    fn test_verify_password_checks_the_active_users_master_password() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        manager.register("Frank".to_string(), "CorrectHorse").unwrap();

        manager.verify_password("CorrectHorse").unwrap();

        let wrong = manager.verify_password("WrongHorse");
        assert!(
            matches!(wrong, Err(VaultError::AuthenticationFailed)),
            "got {wrong:?}"
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_owns_vault_reflects_the_active_users_vault_list() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        manager.register("Grace".to_string(), "Password").unwrap();

        let owned = Uuid::new_v4();
        let foreign = Uuid::new_v4();
        manager.add_vault_to_user(owned).unwrap();

        assert!(manager.owns_vault(owned).unwrap());
        assert!(!manager.owns_vault(foreign).unwrap());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_update_profile_rejects_a_remote_icon() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let user = manager.register("Heidi".to_string(), "Password").unwrap();

        let result = manager.update_profile(
            None,
            None,
            Some("https://attacker.example/x.png".to_string()),
        );

        assert!(
            matches!(result, Err(VaultError::Validation(_))),
            "got {result:?}"
        );
        assert_eq!(manager.get_user(&user.id).unwrap().icon, None);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_user_reads_withhold_a_tampered_icon() {
        let temp_dir = setup();
        let mut manager = UserManager::new(temp_dir.clone()).unwrap();
        let user = manager.register("Ivan".to_string(), "Password").unwrap();

        // A tampered profile file can carry any stored icon string.
        let path = temp_dir.join(format!("{}.json", user.id));
        let mut stored: User = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        stored.icon = Some("https://attacker.example/x.png".to_string());
        std::fs::write(&path, serde_json::to_string(&stored).unwrap()).unwrap();

        assert_eq!(manager.get_user(&user.id).unwrap().icon, None);
        assert_eq!(
            manager.get_users().unwrap()[0].icon,
            None,
            "the user list must withhold a disallowed icon too"
        );

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
