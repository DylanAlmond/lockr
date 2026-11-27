use std::println;
use std::sync::Mutex;

use tauri::State;
use uuid::Uuid;
use zeroize::Zeroize;
use zxcvbn::{zxcvbn, Entropy};

use crate::user_manager::UserManager;
use crate::vault_manager::VaultManager;
use crate::{
    ItemFilter, IntoSafe, NewItemData, SafeItem, SafeVault, SecretInput, User, UserId, VaultError,
    VaultId,
};

pub struct ManagerState {
    pub vault_manager: VaultManager,
    pub user_manager: UserManager,

    /// Master password — only needed for create_vault / unlock_vault.
    /// Held in memory so the user can unlock additional vaults without
    /// re-entering their password. Zeroized on logout.
    pub master_password: Option<String>,
    pub secret_key: Option<Vec<u8>>,
}

type AppState<'a> = State<'a, Mutex<ManagerState>>;

impl ManagerState {
    /// Delete a vault the active user owns, after re-checking the master
    /// password.
    ///
    /// A compromised renderer must not be able to destroy vaults with a
    /// single IPC call, so deletion is gated on ownership and on a fresh
    /// password verification rather than on the cached session alone.
    pub fn delete_vault(
        &mut self,
        vault_id: VaultId,
        master_password: &str,
    ) -> Result<(), VaultError> {
        if !self.user_manager.owns_vault(vault_id)? {
            return Err(VaultError::Unauthorised);
        }

        self.user_manager.verify_password(master_password)?;

        // Move the file aside first, then record the deletion. If recording
        // fails, put the file back rather than leaving an orphaned tombstone
        // the user's profile still points at.
        let tombstone = self.vault_manager.delete_vault(vault_id)?;
        if let Err(error) = self.user_manager.remove_vault_from_user(vault_id) {
            let _ = self.vault_manager.restore_vault(vault_id, &tombstone);
            return Err(error);
        }

        Ok(())
    }
}

#[tauri::command]
pub fn register_user(
    state: AppState,
    name: String,
    master_password: String,
) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        user_manager,
        master_password: mp_state,
        secret_key: sk_state,
    } = &mut *state;

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
    Ok(state.user_manager.get_users().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user(state: AppState) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let user_id = state
        .user_manager
        .active_user_id
        .as_ref()
        .ok_or_else(|| "No user is active".to_string())?;

    println!("id: {}", user_id);

    Ok(state
        .user_manager
        .get_user(user_id)
        .map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user_by_id(state: AppState, id: String) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let user_id: UserId = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    Ok(state
        .user_manager
        .get_user(&user_id)
        .map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn update_profile(
    state: AppState,
    name: Option<String>,
    color: Option<String>,
    icon: Option<String>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state
        .user_manager
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

    let ManagerState {
        vault_manager,
        user_manager,
        master_password: mp_state,
        secret_key: sk_state,
    } = &mut *state;

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

    state.user_manager.logout().map_err(|e| e.to_string())?;

    // Flush any unsaved changes before locking
    state.vault_manager.flush_all().map_err(|e| e.to_string())?;

    state.vault_manager.lock_vault(None);

    if let Some(mut mp) = state.master_password.take() {
        mp.zeroize();
    }
    if let Some(mut sk) = state.secret_key.take() {
        sk.zeroize();
    }

    Ok(())
}

#[tauri::command]
pub fn delete_user(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    state.vault_manager.lock_vault(None);

    if let Some(mut mp) = state.master_password.take() {
        mp.zeroize();
    }
    if let Some(mut sk) = state.secret_key.take() {
        sk.zeroize();
    }

    state.user_manager.delete_user().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_vault_ids(state: AppState) -> Result<Vec<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    state
        .vault_manager
        .list_vault_ids()
        .map_err(|e| e.to_string())
}

/// Returns metadata for all vaults currently sitting in RAM
#[tauri::command]
pub fn get_unlocked_vaults(state: AppState) -> Result<Vec<SafeVault>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state.vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn create_vault(state: AppState, name: String, color: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        user_manager,
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let vault = state
        .vault_manager
        .get_vault(id)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn unlock_vault(state: AppState, vault_id: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    // Vue sends a string but Rust needs a Uuid
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        ..
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

    let vault = vault_manager
        .unlock_vault(id, mp, sk)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn lock_vault(state: AppState, vault_id: Option<String>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = vault_id
        .map(|vid| Uuid::parse_str(&vid).map_err(|e| e.to_string()))
        .transpose()?;

    state.vault_manager.lock_vault(id);
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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .update_vault(id, name, color)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_vault(
    state: AppState,
    vault_id: String,
    master_password: String,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state
        .delete_vault(id, &master_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_password_strength(
    state: AppState,
    vault_id: String,
    item_id: String,
) -> Result<Entropy, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    state
        .vault_manager
        .flush_vault(id)
        .map_err(|e| e.to_string())
}

/// Flush ALL dirty vaults to disk. Call this before backgrounding,
/// navigating away, or periodically as a safety net.
#[tauri::command]
pub fn flush_all(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.vault_manager.flush_all().map_err(|e| e.to_string())
}

/// Toggle autosave on/off.
/// When off, mutations only mark vaults dirty — you must call
/// flush_vault / flush_all to persist.
#[tauri::command]
pub fn set_autosave(state: AppState, enabled: bool) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.vault_manager.set_autosave(enabled);
    Ok(())
}

/// Check if a vault has unsaved changes.
#[tauri::command]
pub fn is_vault_dirty(state: AppState, vault_id: String) -> Result<bool, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    Ok(state.vault_manager.is_dirty(id))
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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .update_item(vid, iid, display_name, icon, favourite, tags, color, data)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_item(state: AppState, vault_id: String, item_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;
    let tid = Uuid::parse_str(&target_vault_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .move_item(vid, iid, tid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_by_id(state: AppState, item_id: String) -> Result<SafeItem, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_item_by_id(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_all_items(state: AppState, filter: ItemFilter) -> Result<Vec<SafeItem>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .update_item_secret(vid, iid, &field_key, value)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const MASTER_PW: &str = "CorrectHorseBattery";
    const SECRET_KEY: [u8; 32] = [7u8; 32];

    /// A logged-in user with one owned vault and the raw managers, so the
    /// deletion preconditions can be exercised without a Tauri harness.
    fn setup() -> (ManagerState, VaultId, PathBuf) {
        let temp_dir = std::env::temp_dir().join(format!("commands_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut user_manager = UserManager::new(temp_dir.clone()).unwrap();
        user_manager
            .register("Alice".to_string(), MASTER_PW)
            .unwrap();

        let mut vault_manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = vault_manager
            .create_vault(
                "Personal".to_string(),
                "#6240BF".to_string(),
                MASTER_PW,
                &SECRET_KEY,
            )
            .unwrap();
        user_manager.add_vault_to_user(vault.id).unwrap();

        let state = ManagerState {
            vault_manager,
            user_manager,
            master_password: Some(MASTER_PW.to_string()),
            secret_key: Some(SECRET_KEY.to_vec()),
        };

        (state, vault.id, temp_dir)
    }

    #[test]
    fn test_delete_vault_requires_the_master_password() {
        let (mut state, vault_id, temp_dir) = setup();

        let result = state.delete_vault(vault_id, "WrongPassword");

        assert!(
            matches!(result, Err(VaultError::AuthenticationFailed)),
            "got {result:?}"
        );
        assert!(
            temp_dir
                .join("vaults")
                .join(format!("{}.vault", vault_id))
                .exists(),
            "a failed re-authentication must leave the vault in place"
        );
        assert!(
            state.user_manager.owns_vault(vault_id).unwrap(),
            "a failed re-authentication must not touch the user's vault list"
        );
    }

    #[test]
    fn test_delete_vault_rejects_a_vault_the_active_user_does_not_own() {
        let (mut state, _, temp_dir) = setup();
        let foreign = state
            .vault_manager
            .create_vault(
                "Foreign".to_string(),
                "#000000".to_string(),
                MASTER_PW,
                &SECRET_KEY,
            )
            .unwrap();

        let result = state.delete_vault(foreign.id, MASTER_PW);

        assert!(
            matches!(result, Err(VaultError::Unauthorised)),
            "got {result:?}"
        );
        assert!(
            temp_dir
                .join("vaults")
                .join(format!("{}.vault", foreign.id))
                .exists(),
            "an unowned vault must not be deleted"
        );
    }

    #[test]
    fn test_delete_vault_tombstones_an_owned_vault() {
        let (mut state, vault_id, temp_dir) = setup();

        state.delete_vault(vault_id, MASTER_PW).unwrap();

        let vaults_dir = temp_dir.join("vaults");
        assert!(
            !vaults_dir.join(format!("{}.vault", vault_id)).exists(),
            "the live vault file must be moved aside"
        );
        assert!(
            vaults_dir
                .join(format!("{}.vault.deleted", vault_id))
                .exists(),
            "the deleted vault must be recoverable"
        );
        assert!(
            !state.user_manager.owns_vault(vault_id).unwrap(),
            "deletion must remove the vault from the user's list"
        );
    }
}
