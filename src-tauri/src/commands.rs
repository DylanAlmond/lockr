use std::sync::Mutex;

use tauri::State;
use uuid::Uuid;
use zeroize::Zeroize;
use zxcvbn::{zxcvbn, Entropy, Score};

use crate::user_manager::UserManager;
use crate::vault_manager::VaultManager;
use crate::{AccountFilter, IntoSafe, SafeAccount, SafeVault, User};

pub struct ManagerState {
    pub vault_manager: VaultManager,
    pub user_manager: UserManager,

    pub master_password: Option<String>,
    pub secret_key: Option<Vec<u8>>,
}

type AppState<'a> = State<'a, Mutex<ManagerState>>;

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

    user_manager
        .register(name, &master_password)
        .map_err(|e| e.to_string())?;

    let sk = user_manager
        .login(&master_password)
        .map_err(|e| e.to_string())?;

    *mp_state = Some(master_password);
    *sk_state = Some(sk);

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    // Create default vault
    let new_vault = vault_manager
        .create_vault("Personal".to_string(), mp, sk)
        .map_err(|e| e.to_string())?;

    user_manager
        .add_vault_to_user(new_vault.id)
        .map_err(|e| e.to_string())?;

    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn get_user(state: AppState) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state.user_manager.get_user().map_err(|e| e.to_string())?)
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
pub fn login_user(state: AppState, master_password: String) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        user_manager,
        master_password: mp_state,
        secret_key: sk_state,
    } = &mut *state;

    let sk = user_manager
        .login(&master_password)
        .map_err(|e| e.to_string())?;

    *mp_state = Some(master_password);
    *sk_state = Some(sk);

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    let user = user_manager.get_user().map_err(|e| e.to_string())?;

    for vault_id in &user.vault_ids {
        let _ = vault_manager.unlock_vault(*vault_id, mp, sk);
    }

    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn logout(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

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
pub fn create_vault(state: AppState, name: String) -> Result<SafeVault, String> {
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
        .create_vault(name, mp, sk)
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

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        ..
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

    vault_manager
        .update_vault(id, name, color, mp, sk)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state
        .user_manager
        .remove_vault_from_user(id)
        .map_err(|e| e.to_string())?;

    state
        .vault_manager
        .delete_vault(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_account(
    state: AppState,
    vault_id: String,
    display_name: Option<String>,
    username: String,
    email: Option<String>,
    password: String,
) -> Result<SafeAccount, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        user_manager: _,
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

    vault_manager
        .add_account(id, display_name, username, email, password, mp, sk)
        .map(|a| a.into_safe())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_account_by_id(state: AppState, account_id: String) -> Result<SafeAccount, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_account_by_id(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_all_accounts(
    state: AppState,
    filter: AccountFilter,
) -> Result<Vec<SafeAccount>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_all_accounts(filter)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_account(
    state: AppState,
    vault_id: String,
    account_id: String,
    display_name: Option<String>,
    username: Option<String>,
    email: Option<String>,
    favourite: Option<bool>,
    tags: Option<Vec<String>>,
    icon: Option<String>,
    color: Option<String>,
    password: Option<String>,
) -> Result<SafeAccount, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        ..
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

    vault_manager
        .update_account(
            vid,
            aid,
            display_name,
            username,
            email,
            favourite,
            tags,
            icon,
            color,
            password,
            mp,
            sk,
        )
        .map(|a| a.into_safe())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_account(state: AppState, vault_id: String, account_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        ..
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

    vault_manager
        .delete_account(vid, aid, mp, sk)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_secret(state: AppState, vault_id: String, account_id: String) -> Result<String, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_secret(vid, aid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_account_password_strength(
    state: AppState,
    vault_id: String,
    account_id: String,
) -> Result<Entropy, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_account_password_strength(vid, aid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_password_strength(password: String) -> Result<Entropy, String> {
    Ok(zxcvbn(&password, &[]).into())
}
