use std::sync::Mutex;

use tauri::State;
use uuid::Uuid;

use crate::user_manager::UserManager;
use crate::vault_manager::VaultManager;
use crate::{AccountFilter, IntoSafe, SafeAccount, SafeVault, User};

pub struct ManagerState {
    pub vault_manager: VaultManager,
    pub user_manager: UserManager,
}

type AppState<'a> = State<'a, Mutex<ManagerState>>;

#[tauri::command]
pub fn get_user(state: AppState) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state.user_manager.get_user().clone())
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
pub fn is_vault_unlocked(state: AppState, vault_id: String) -> Result<bool, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    Ok(state.vault_manager.is_vault_unlocked(id))
}

#[tauri::command]
pub fn is_any_unlocked(state: AppState) -> Result<bool, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state.vault_manager.is_any_unlocked())
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
pub fn create_vault(
    state: AppState,
    name: String,
    master_password: String,
) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let vault = state
        .vault_manager
        .create_vault(name, master_password)
        .map_err(|e| e.to_string())?;

    state
        .user_manager
        .set_active_vault(Some(vault.id))
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn unlock_vault(
    state: AppState,
    vault_id: String,
    master_password: String,
) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    // Vue sends a string but Rust needs a Uuid
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let vault = state
        .vault_manager
        .unlock_vault(id, master_password)
        .map_err(|e| e.to_string())?;

    state
        .user_manager
        .set_active_vault(Some(vault.id))
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn lock_vault(state: AppState, vault_id: Option<String>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = vault_id
        .map(|vid| uuid::Uuid::parse_str(&vid).map_err(|e| e.to_string()))
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
    let id = uuid::Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .update_vault(id, name, color)
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
    let id = uuid::Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .add_account(id, display_name, username, email, password)
        .map(|a| a.into_safe())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_account(
    state: AppState,
    vault_id: String,
    account_id: String,
) -> Result<SafeAccount, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let vid = uuid::Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = uuid::Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_account(vid, aid)
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
    let vid = uuid::Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = uuid::Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
        )
        .map(|a| a.into_safe())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_account(state: AppState, vault_id: String, account_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let vid = uuid::Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = uuid::Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .delete_account(vid, aid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_secret(state: AppState, vault_id: String, account_id: String) -> Result<String, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let vid = uuid::Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let aid = uuid::Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_secret(vid, aid)
        .map_err(|e| e.to_string())
}
