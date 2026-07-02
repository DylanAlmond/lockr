mod commands;
mod crypto;
mod error;
mod models;
mod vault_manager;

use std::sync::Mutex;

pub use error::VaultError;
pub use models::*;
use tauri::Manager;

use crate::vault_manager::VaultManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data dir");

            let manager =
                VaultManager::new(app_data_dir).expect("Failed to initialize VaultManager");

            app.manage(Mutex::new(manager));

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::create_vault,
            commands::list_vault_ids,
            commands::unlock_vault,
            commands::lock_vault,
            commands::add_service,
            commands::delete_service,
            commands::add_account,
            commands::update_account,
            commands::delete_account,
            commands::update_vault_name,
            commands::update_service_name,
            commands::get_secret
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
