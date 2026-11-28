use argon2::password_hash::rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    println,
    time::{Duration, SystemTime, UNIX_EPOCH},
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
const LOGIN_BACKOFF_BASE: Duration = Duration::from_secs(5 * 60);
const LOGIN_BACKOFF_MAX: Duration = Duration::from_secs(15 * 60);

struct LoginThrottle {
    failures: u32,
    blocked_until: Option<SystemTime>,
}

/// On-disk form. `SystemTime` is not serializable, so the block is stored as a
/// Unix timestamp.
#[derive(Serialize, Deserialize)]
struct StoredThrottle {
    failures: u32,
    blocked_until_unix: Option<u64>,
}

pub struct UserManager {
    app_data_dir: PathBuf,
    pub active_user_id: Option<UserId>,
    active_path: Option<PathBuf>,

    /// Consecutive failed logins per user, persisted so restarting the app
    /// cannot clear an active lockout. The file is not a user profile and has
    /// a non-`.json` name so `get_users` never scans it.
    throttle: HashMap<UserId, LoginThrottle>,
}

/// The throttle state file. It is a `.dat` file holding JSON so `get_users`
/// (which scans `*.json` profiles) never reads it as a profile.
fn throttle_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("login-throttle.dat")
}

fn load_throttle(app_data_dir: &Path) -> HashMap<UserId, LoginThrottle> {
    let Ok(json) = std::fs::read_to_string(throttle_path(app_data_dir)) else {
        return HashMap::new();
    };
    let Ok(stored) = serde_json::from_str::<HashMap<String, StoredThrottle>>(&json) else {
        return HashMap::new();
    };

    stored
        .into_iter()
        .filter_map(|(id, entry)| {
            let id = Uuid::parse_str(&id).ok()?;
            Some((
                id,
                LoginThrottle {
                    failures: entry.failures,
                    blocked_until: entry
                        .blocked_until_unix
                        .map(|secs| UNIX_EPOCH + Duration::from_secs(secs)),
                },
            ))
        })
        .collect()
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

/// Human-readable retry delay for the throttle message.
fn format_retry_delay(remaining: Duration) -> String {
    let seconds = remaining.as_secs().max(1);
    let minutes = seconds / 60;
    let secs = seconds % 60;

    match (minutes, secs) {
        (0, secs) => format!("{secs} second{}", plural(secs)),
        (minutes, 0) => format!("{minutes} minute{}", plural(minutes)),
        (minutes, secs) => format!(
            "{minutes} minute{} {secs} second{}",
            plural(minutes),
            plural(secs)
        ),
    }
}

fn plural(value: u64) -> &'static str {
    if value == 1 {
        ""
    } else {
        "s"
    }
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

        let throttle = load_throttle(&app_data_dir);

        Ok(Self {
            app_data_dir,
            active_user_id: None,
            active_path: None,
            throttle,
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
    fn log_auth_event(&self, event: &str, user: &UserId) {
        let timestamp = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        let line = format!("{timestamp} {event} user={user}\n");

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

        let now = SystemTime::now();
        if blocked_until > now {
            let remaining = blocked_until.duration_since(now).unwrap_or_default();
            return Err(VaultError::TooManyAttempts(format_retry_delay(remaining)));
        }

        Ok(())
    }

    /// Seconds until the next login is allowed, if a block is active.
    pub fn cooldown_remaining(&self, id: &UserId) -> Option<u64> {
        let blocked_until = self.throttle.get(id)?.blocked_until?;
        let remaining = blocked_until.duration_since(SystemTime::now()).ok()?;

        if remaining.is_zero() {
            None
        } else {
            Some(remaining.as_secs().max(1))
        }
    }

    fn record_failed_login(&mut self, id: &UserId) {
        {
            let throttle = self.throttle.entry(*id).or_insert(LoginThrottle {
                failures: 0,
                blocked_until: None,
            });
            throttle.failures = throttle.failures.saturating_add(1);

            if let Some(delay) = login_backoff(throttle.failures) {
                throttle.blocked_until = Some(SystemTime::now() + delay);
            }
        }

        self.save_throttle();
    }

    fn record_successful_login(&mut self, id: &UserId) {
        self.throttle.remove(id);
        self.save_throttle();
    }

    fn save_throttle(&self) {
        let stored: HashMap<String, StoredThrottle> = self
            .throttle
            .iter()
            .map(|(id, entry)| {
                (
                    id.to_string(),
                    StoredThrottle {
                        failures: entry.failures,
                        blocked_until_unix: entry.blocked_until.and_then(|until| {
                            until
                                .duration_since(UNIX_EPOCH)
                                .ok()
                                .map(|duration| duration.as_secs())
                        }),
                    },
                )
            })
            .collect();

        let Ok(json) = serde_json::to_vec(&stored) else {
            return;
        };

        let path = throttle_path(&self.app_data_dir);
        let temp = path.with_extension("tmp");
        let _ = std::fs::write(&temp, json).and_then(|_| std::fs::rename(&temp, &path));
    }

    /// Create a brand new user profile.
    /// Generates a random Secret Key and encrypts it with the Master Password.
    pub fn register(&mut self, name: String, master_password: &str) -> Result<User, VaultError> {
        // The profile-creation layer enforces the policy so no caller can
        // bypass it; the command layer also checks early to preserve the
        // current session on rejection.
        validate_master_password(&name, master_password)?;

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

        // Write the profile before making it active: a failed write must not
        // leave an active user pointing at a missing file.
        let path = self
            .app_data_dir
            .join(format!("{}.json", user.id.to_string()));
        std::fs::write(&path, serde_json::to_string_pretty(&user)?)?;

        self.active_user_id = Some(user.id);
        self.active_path = Some(path);
        self.log_auth_event("registered", &user.id);

        Ok(user)
    }

    /// Verify the Master Password and decrypt the Secret Key.
    pub fn login(&mut self, id: &UserId, master_password: &str) -> Result<Vec<u8>, VaultError> {
        if let Err(error) = self.check_login_allowed(id) {
            self.log_auth_event("login_blocked", id);
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
                    self.log_auth_event("login_failed", id);
                    return Err(error);
                }
            };

        self.record_successful_login(id);
        self.log_auth_event("login_success", id);

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
            self.log_auth_event("reauth_blocked", &user.id);
            return Err(error);
        }

        let mut secret_key =
            match Self::decrypt_secret_key(master_password, &user.encrypted_secret_key) {
                Ok(key) => key,
                Err(error) => {
                    self.record_failed_login(&user.id);
                    self.log_auth_event("reauth_failed", &user.id);
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
        assert_eq!(login_backoff(5), Some(Duration::from_secs(5 * 60)));
        assert_eq!(login_backoff(6), Some(Duration::from_secs(10 * 60)));
        assert_eq!(login_backoff(7), Some(LOGIN_BACKOFF_MAX));
        assert_eq!(login_backoff(20), Some(LOGIN_BACKOFF_MAX));
    }

    #[test]
    fn test_retry_delay_is_human_readable() {
        assert_eq!(format_retry_delay(Duration::from_secs(300)), "5 minutes");
        assert_eq!(
            format_retry_delay(Duration::from_secs(150)),
            "2 minutes 30 seconds"
        );
        assert_eq!(format_retry_delay(Duration::from_secs(1)), "1 second");
        assert_eq!(format_retry_delay(Duration::from_secs(60)), "1 minute");
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
        assert!(manager.cooldown_remaining(&id).is_some());

        manager.record_successful_login(&id);
        manager.check_login_allowed(&id).unwrap();
        assert!(manager.cooldown_remaining(&id).is_none());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_login_throttle_survives_restart() {
        let temp_dir = setup();
        let id = Uuid::new_v4();

        {
            let mut manager = UserManager::new(temp_dir.clone()).unwrap();
            for _ in 0..5 {
                manager.record_failed_login(&id);
            }
        }

        let manager = UserManager::new(temp_dir.clone()).unwrap();
        assert!(
            manager.cooldown_remaining(&id).is_some(),
            "a restart must not clear an active lockout"
        );
        assert!(matches!(
            manager.check_login_allowed(&id),
            Err(VaultError::TooManyAttempts(_))
        ));
        assert_eq!(
            manager.get_users().unwrap().len(),
            0,
            "the throttle file must not be read as a profile"
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_auth_log_records_events_without_secrets() {
        let temp_dir = setup();
        let manager = UserManager::new(temp_dir.clone()).unwrap();
        let id = Uuid::new_v4();
        let secret = "correct horse battery staple";

        manager.log_auth_event("login_failed", &id);

        let log = std::fs::read_to_string(temp_dir.join("auth.log")).unwrap();
        assert!(log.contains("login_failed"));
        assert!(log.contains(&id.to_string()));
        assert!(!log.contains(secret), "the log must never contain a secret");

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
            .register("Charlie".to_string(), "correct horse battery staple")
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
        manager.register("Frank".to_string(), "correct horse battery staple").unwrap();

        manager.verify_password("correct horse battery staple").unwrap();

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
        manager.register("Grace".to_string(), "correct horse battery staple").unwrap();

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
        let user = manager.register("Heidi".to_string(), "correct horse battery staple").unwrap();

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
        let user = manager.register("Ivan".to_string(), "correct horse battery staple").unwrap();

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
            .register("Dave".to_string(), "correct horse battery staple")
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

        let user = manager.register("Eve".to_string(), "correct horse battery staple").unwrap();

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
