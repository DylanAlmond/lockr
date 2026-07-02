use std::sync::Mutex;

use tauri::State;
use uuid::Uuid;

use crate::{vault_manager::VaultManager, Vault};

type ManagerState<'a> = State<'a, Mutex<VaultManager>>;

#[tauri::command]
pub fn create_vault(
    state: ManagerState,
    name: String,
    master_password: String,
) -> Result<Vault, String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;

    manager
        .create_vault(name, master_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_vault_ids(state: ManagerState) -> Result<Vec<String>, String> {
    let manager = state.lock().map_err(|e| e.to_string())?;
    manager.list_vault_ids().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn unlock_vault(
    state: ManagerState,
    vault_id: String,
    master_password: String,
) -> Result<Vault, String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;

    // Vue sends a string but Rust needs a Uuid
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    manager
        .unlock_vault(id, master_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_vault_name(state: ManagerState, new_name: String) -> Result<(), String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;
    manager
        .update_vault_name(new_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn lock_vault(state: ManagerState) -> Result<(), String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;
    manager.lock_vault();
    Ok(())
}

#[tauri::command]
pub fn add_service(state: ManagerState, name: String) -> Result<crate::models::Service, String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;
    manager.add_service(name).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_service_name(
    state: ManagerState,
    service_id: String,
    new_name: String,
) -> Result<(), String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;
    let id = uuid::Uuid::parse_str(&service_id).map_err(|e| e.to_string())?;
    manager
        .update_service_name(id, new_name)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_service(state: ManagerState, service_id: String) -> Result<(), String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&service_id).map_err(|e| e.to_string())?;
    manager.delete_service(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_account(
    state: ManagerState,
    service_id: String,
    display_name: Option<String>,
    username: String,
    email: Option<String>,
    password: String,
) -> Result<crate::models::Account, String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&service_id).map_err(|e| e.to_string())?;
    manager
        .add_account(id, display_name, username, email, password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_account(
    state: ManagerState,
    service_id: String,
    account_id: String,
    display_name: Option<String>,
    username: Option<String>,
    email: Option<String>,
    password: Option<String>,
) -> Result<crate::models::Account, String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;

    let sid = Uuid::parse_str(&service_id).map_err(|e| e.to_string())?;
    let aid = Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    manager
        .update_account(sid, aid, display_name, username, email, password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_account(
    state: ManagerState,
    service_id: String,
    account_id: String,
) -> Result<(), String> {
    let mut manager = state.lock().map_err(|e| e.to_string())?;

    let sid = Uuid::parse_str(&service_id).map_err(|e| e.to_string())?;
    let aid = Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;

    manager.delete_account(sid, aid).map_err(|e| e.to_string())
}
