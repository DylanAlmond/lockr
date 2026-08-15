mod commands;
mod crypto;
mod error;
mod models;
mod user_manager;
mod vault_manager;

use std::sync::Mutex;
#[cfg(target_os = "windows")]
use window_vibrancy::apply_tabbed;

pub use error::VaultError;
pub use models::*;
use tauri::Manager;

use crate::{commands::ManagerState, user_manager::UserManager, vault_manager::VaultManager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let window = app
                .get_webview_window("main")
                .expect("main window not found");

            #[cfg(target_os = "windows")]
            apply_tabbed(&window, Some(false))
                .expect("Unsupported platform! 'apply_blur' is only supported on Windows");

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
                master_password: None,
                secret_key: None,
            };

            app.manage(Mutex::new(app_state));

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            // User
            commands::register_user,
            commands::login_user,
            commands::get_user,
            commands::update_profile,
            commands::logout,
            commands::delete_user,
            // Vaults
            commands::list_vault_ids,
            commands::get_unlocked_vaults,
            commands::get_vault_by_id,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::update_vault,
            commands::delete_vault,
            // Accounts
            commands::add_account,
            commands::get_account_by_id,
            commands::get_all_accounts,
            commands::update_account,
            commands::delete_account,
            commands::get_secret,
            commands::get_account_password_strength,
            // Util
            commands::get_password_strength,
            commands::flush_vault,
            commands::flush_all,
            commands::set_autosave,
            commands::is_vault_dirty
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
