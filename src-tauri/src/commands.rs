use std::path::PathBuf;
use std::println;
use std::sync::Mutex;

use tauri::State;
use uuid::Uuid;
use zeroize::Zeroize;
use zxcvbn::{zxcvbn, Entropy};

use crate::user_manager::UserManager;
use crate::vault_manager::VaultManager;
use crate::{IntoSafe, ItemFilter, NewItemData, SafeItem, SafeVault, SecretInput, User, UserId};

pub struct ManagerState {
    /// Where the managers read and write. Kept so an explicit identity
    /// recovery can re-initialise them without restarting the process.
    pub app_data_dir: PathBuf,

    /// `None` while a damaged Device identity is blocking startup. No command
    /// may touch vaults or profiles until the User chooses a recovery.
    pub vault_manager: Option<VaultManager>,
    pub user_manager: Option<UserManager>,

    /// Master password — only needed for create_vault / unlock_vault.
    /// Held in memory so the user can unlock additional vaults without
    /// re-entering their password. Zeroized on logout.
    pub master_password: Option<String>,
    pub secret_key: Option<Vec<u8>>,

    /// A distinct, user-visible startup failure. Today this is only a damaged
    /// Device identity file; it is surfaced instead of panicking the process.
    pub startup_error: Option<String>,

    /// Whether the failure is specifically a damaged Device identity, the one
    /// case where starting with a new identity is a sensible way out.
    pub device_identity_damaged: bool,
}

impl ManagerState {
    /// Build the shared state. A damaged Device identity leaves the managers
    /// unset and records a recoverable error rather than aborting the app.
    pub fn initialize(app_data_dir: PathBuf) -> Self {
        match (
            VaultManager::new(app_data_dir.clone()),
            UserManager::new(app_data_dir.clone()),
        ) {
            (Ok(vault_manager), Ok(user_manager)) => Self {
                app_data_dir,
                vault_manager: Some(vault_manager),
                user_manager: Some(user_manager),
                master_password: None,
                secret_key: None,
                startup_error: None,
                device_identity_damaged: false,
            },
            (vault_result, user_result) => {
                let device_identity_damaged = matches!(
                    &vault_result,
                    Err(crate::VaultError::DeviceIdentityDamaged(_))
                ) || matches!(
                    &user_result,
                    Err(crate::VaultError::DeviceIdentityDamaged(_))
                );

                let error = vault_result
                    .err()
                    .or_else(|| user_result.err())
                    .map(|err| err.to_string())
                    .unwrap_or_else(|| "Lockr could not initialize".to_string());

                Self {
                    app_data_dir,
                    vault_manager: None,
                    user_manager: None,
                    master_password: None,
                    secret_key: None,
                    startup_error: Some(error),
                    device_identity_damaged,
                }
            }
        }
    }

    fn startup_message(&self) -> String {
        self.startup_error
            .clone()
            .unwrap_or_else(|| "Lockr is not initialized".to_string())
    }

    /// Abandon a damaged Device identity and rebuild the managers under a
    /// freshly minted one. One-way by construction: the caller must already
    /// have obtained explicit confirmation.
    pub fn recover_with_new_device_identity(&mut self) -> Result<(), String> {
        if !self.device_identity_damaged {
            return Err("This Device's identity is not damaged".into());
        }

        let app_data_dir = self.app_data_dir.clone();
        crate::device::reset_device_id(&app_data_dir).map_err(|e| e.to_string())?;

        *self = ManagerState::initialize(app_data_dir);

        match &self.startup_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn ready(&self) -> Result<(&VaultManager, &UserManager), String> {
        match (&self.vault_manager, &self.user_manager) {
            (Some(vault_manager), Some(user_manager)) => Ok((vault_manager, user_manager)),
            _ => Err(self.startup_message()),
        }
    }

    fn ready_mut(&mut self) -> Result<(&mut VaultManager, &mut UserManager), String> {
        let message = self.startup_message();
        match (&mut self.vault_manager, &mut self.user_manager) {
            (Some(vault_manager), Some(user_manager)) => Ok((vault_manager, user_manager)),
            _ => Err(message),
        }
    }

    fn parts_mut(
        &mut self,
    ) -> Result<
        (
            &mut VaultManager,
            &mut UserManager,
            &mut Option<String>,
            &mut Option<Vec<u8>>,
        ),
        String,
    > {
        let message = self.startup_message();
        match (&mut self.vault_manager, &mut self.user_manager) {
            (Some(vault_manager), Some(user_manager)) => Ok((
                vault_manager,
                user_manager,
                &mut self.master_password,
                &mut self.secret_key,
            )),
            _ => Err(message),
        }
    }
}

type AppState<'a> = State<'a, Mutex<ManagerState>>;

#[tauri::command]
pub fn register_user(
    state: AppState,
    name: String,
    master_password: String,
) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    let user = user_manager
        .register(name, &master_password)
        .map_err(|e| e.to_string())?;

    let sk = user_manager
        .login(&user.id, &master_password)
        .map_err(|e| e.to_string())?;

    *mp_state = Some(master_password);
    *sk_state = Some(sk);

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    // Create default vault
    let new_vault = vault_manager
        .create_vault("Personal".to_string(), "#6240BF".to_string(), mp, sk)
        .map_err(|e| e.to_string())?;

    user_manager
        .add_vault_to_user(new_vault.id)
        .map_err(|e| e.to_string())?;

    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn get_users(state: AppState) -> Result<Vec<User>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    Ok(user_manager.get_users().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user(state: AppState) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let user_id = user_manager
        .active_user_id
        .as_ref()
        .ok_or_else(|| "No user is active".to_string())?;

    println!("id: {}", user_id);

    Ok(user_manager.get_user(user_id).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user_by_id(state: AppState, id: String) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let user_id: UserId = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    Ok(user_manager.get_user(&user_id).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn update_profile(
    state: AppState,
    name: Option<String>,
    color: Option<String>,
    icon: Option<String>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready_mut()?;
    user_manager
        .update_profile(name, color, icon)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn login_user(
    state: AppState,
    id: String,
    master_password: String,
) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    let user_id: UserId = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    let sk = user_manager
        .login(&user_id, &master_password)
        .map_err(|e| e.to_string())?;

    *mp_state = Some(master_password);
    *sk_state = Some(sk);

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    let user = user_manager.get_user(&user_id).map_err(|e| e.to_string())?;

    // Unlock all vaults — each derives its master key once (Argon2id).
    // After this, all further operations use the cached keys.
    for vault_id in &user.vault_ids {
        let _ = vault_manager.unlock_vault(*vault_id, mp, sk);
    }

    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn logout(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    user_manager.logout().map_err(|e| e.to_string())?;

    // Flush any unsaved changes before locking
    vault_manager.flush_all().map_err(|e| e.to_string())?;

    vault_manager.lock_vault(None);

    if let Some(mut mp) = mp_state.take() {
        mp.zeroize();
    }
    if let Some(mut sk) = sk_state.take() {
        sk.zeroize();
    }

    Ok(())
}

#[tauri::command]
pub fn delete_user(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    vault_manager.lock_vault(None);

    if let Some(mut mp) = mp_state.take() {
        mp.zeroize();
    }
    if let Some(mut sk) = sk_state.take() {
        sk.zeroize();
    }

    user_manager.delete_user().map_err(|e| e.to_string())
}

/// The distinct startup failure blocking normal use, if any. A damaged
/// Device identity file is reported here instead of panicking the process.
#[tauri::command]
pub fn get_device_identity_error(state: AppState) -> Result<Option<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state.startup_error.clone())
}

/// Explicitly start again as a new Device after a damaged identity file.
///
/// This is one-way: the old Device id is retired permanently. The damaged
/// file is kept beside the new one for inspection. `confirm` exists so the
/// action can never be taken by an accidental click.
#[tauri::command]
pub fn start_with_new_device_identity(state: AppState, confirm: bool) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    if !state.device_identity_damaged {
        return Err("This Device's identity is not damaged".into());
    }
    if !confirm {
        return Err(
            "Starting with a new identity is permanent: other Devices will not recognise \
             this one, and the old Device stays revoked. Pass confirm=true to proceed."
                .into(),
        );
    }

    state.recover_with_new_device_identity()
}

#[tauri::command]
pub fn list_vault_ids(state: AppState) -> Result<Vec<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    vault_manager.list_vault_ids().map_err(|e| e.to_string())
}

/// Returns metadata for all vaults currently sitting in RAM
#[tauri::command]
pub fn get_unlocked_vaults(state: AppState) -> Result<Vec<SafeVault>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn create_vault(state: AppState, name: String, color: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    let vault = vault_manager
        .create_vault(name, color, mp, sk)
        .map_err(|e| e.to_string())?;

    user_manager
        .add_vault_to_user(vault.id)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn get_vault_by_id(state: AppState, vault_id: String) -> Result<SafeVault, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let vault = vault_manager.get_vault(id).map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn unlock_vault(state: AppState, vault_id: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    // Vue sends a string but Rust needs a Uuid
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let (vault_manager, _, mp_state, sk_state) = state.parts_mut()?;

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    let vault = vault_manager
        .unlock_vault(id, mp, sk)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn lock_vault(state: AppState, vault_id: Option<String>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let id = vault_id
        .map(|vid| Uuid::parse_str(&vid).map_err(|e| e.to_string()))
        .transpose()?;

    vault_manager.lock_vault(id);
    Ok(())
}

#[tauri::command]
pub fn update_vault(
    state: AppState,
    vault_id: String,
    name: Option<String>,
    color: Option<String>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    vault_manager
        .update_vault(id, name, color)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, user_manager) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    user_manager
        .remove_vault_from_user(id)
        .map_err(|e| e.to_string())?;

    vault_manager.delete_vault(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_password_strength(
    state: AppState,
    vault_id: String,
    item_id: String,
) -> Result<Entropy, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .get_item_password_strength(vid, iid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_password_strength(password: String) -> Result<Entropy, String> {
    Ok(zxcvbn(&password, &[]).into())
}

/// Flush a specific vault's unsaved changes to disk.
/// Call this after batch operations when autosave is off.
#[tauri::command]
pub fn flush_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    vault_manager.flush_vault(id).map_err(|e| e.to_string())
}

/// Flush ALL dirty vaults to disk. Call this before backgrounding,
/// navigating away, or periodically as a safety net.
#[tauri::command]
pub fn flush_all(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    vault_manager.flush_all().map_err(|e| e.to_string())
}

/// Toggle autosave on/off.
/// When off, mutations only mark vaults dirty — you must call
/// flush_vault / flush_all to persist.
#[tauri::command]
pub fn set_autosave(state: AppState, enabled: bool) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    vault_manager.set_autosave(enabled);
    Ok(())
}

/// Check if a vault has unsaved changes.
#[tauri::command]
pub fn is_vault_dirty(state: AppState, vault_id: String) -> Result<bool, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    Ok(vault_manager.is_dirty(id))
}

// ============ Items (the generic mutation/query surface, all kinds) ============

#[tauri::command]
pub fn add_item(
    state: AppState,
    vault_id: String,
    display_name: Option<String>,
    icon: Option<String>,
    data: NewItemData,
    secrets: Option<Vec<SecretInput>>,
) -> Result<SafeItem, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    vault_manager
        .add_item(id, display_name, icon, data, secrets)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_item(
    state: AppState,
    vault_id: String,
    item_id: String,
    display_name: Option<String>,
    icon: Option<String>,
    favourite: Option<bool>,
    tags: Option<Vec<String>>,
    color: Option<String>,
    data: Option<NewItemData>,
) -> Result<SafeItem, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .update_item(vid, iid, display_name, icon, favourite, tags, color, data)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_item(state: AppState, vault_id: String, item_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .delete_item(vid, iid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn move_item(
    state: AppState,
    vault_id: String,
    item_id: String,
    target_vault_id: String,
) -> Result<SafeItem, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;
    let tid = Uuid::parse_str(&target_vault_id).map_err(|e| e.to_string())?;

    vault_manager
        .move_item(vid, iid, tid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_by_id(state: AppState, item_id: String) -> Result<SafeItem, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let id = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager.get_item_by_id(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_all_items(state: AppState, filter: ItemFilter) -> Result<Vec<SafeItem>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;

    vault_manager
        .get_all_items(filter)
        .map_err(|e| e.to_string())
}

// ============ Per-field secrets ============

#[tauri::command]
pub fn get_item_secret(
    state: AppState,
    vault_id: String,
    item_id: String,
    field_key: String,
) -> Result<Option<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .get_item_secret(vid, iid, &field_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_item_secret(
    state: AppState,
    vault_id: String,
    item_id: String,
    field_key: String,
    value: Option<String>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .update_item_secret(vid, iid, &field_key, value)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> PathBuf {
        let temp_dir = std::env::temp_dir().join(format!("lockr_commands_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_a_damaged_device_identity_is_surfaced_without_panicking_or_being_replaced() {
        let temp_dir = setup();
        std::fs::write(temp_dir.join("device_id"), b"not-a-uuid").unwrap();

        let mut state = ManagerState::initialize(temp_dir.clone());

        let error = state
            .startup_error
            .as_ref()
            .expect("a damaged identity must be reported");
        assert!(
            error.contains("Device identity"),
            "unexpected error: {error}"
        );
        assert!(state.device_identity_damaged);
        assert!(state.vault_manager.is_none());
        assert!(state.user_manager.is_none());
        assert!(state.ready().is_err());
        assert_eq!(
            std::fs::read(temp_dir.join("device_id")).unwrap(),
            b"not-a-uuid",
            "startup must not silently replace the identity"
        );

        // Recovery is explicit and one-way, and puts the app back to work.
        state.recover_with_new_device_identity().unwrap();
        assert!(state.startup_error.is_none());
        assert!(state.ready().is_ok());
        assert_ne!(
            std::fs::read(temp_dir.join("device_id")).unwrap(),
            b"not-a-uuid"
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_identity_recovery_is_refused_when_there_is_nothing_to_recover() {
        let temp_dir = setup();
        let mut state = ManagerState::initialize(temp_dir.clone());

        let result = state.recover_with_new_device_identity();

        assert!(result.is_err());
        std::fs::remove_dir_all(temp_dir).ok();
    }
}
