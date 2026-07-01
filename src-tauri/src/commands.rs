use std::sync::Mutex;

use tauri::State;

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
    let id = uuid::Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    manager
        .unlock_vault(id, master_password)
        .map_err(|e| e.to_string())
}
