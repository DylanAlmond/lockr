mod commands;
mod crypto;
mod error;
mod models;
mod user_manager;
mod vault_manager;

use std::sync::Mutex;

pub use error::VaultError;
pub use models::*;
use tauri::Manager;

use crate::{commands::ManagerState, user_manager::UserManager, vault_manager::VaultManager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data dir");

            // Initialize both managers
            let vault_manager =
                VaultManager::new(app_data_dir.clone()).expect("Failed to initialize VaultManager");

            let user_manager =
                UserManager::new(app_data_dir).expect("Failed to initialize UserManager");

            // Bundle them into our shared state
            let app_state = ManagerState {
                vault_manager,
                user_manager,
            };

            app.manage(Mutex::new(app_state));

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            // User
            commands::get_user,
            commands::update_profile,
            // Vaults
            commands::is_vault_unlocked,
            commands::is_any_unlocked,
            commands::list_vault_ids,
            commands::get_unlocked_vaults,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::update_vault,
            commands::delete_vault,
            // Accounts
            commands::add_account,
            commands::get_account,
            commands::get_all_accounts,
            commands::update_account,
            commands::delete_account,
            commands::get_secret,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
