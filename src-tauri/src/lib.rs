mod clipboard;
mod commands;
mod crypto;
mod error;
mod icons;
mod models;
pub mod user_manager;
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
            app.manage(Mutex::new(ManagerState::new(vault_manager, user_manager)));
            commands::spawn_session_monitor(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // User
            commands::get_users,
            commands::register_user,
            commands::login_user,
            commands::get_user,
            commands::get_user_by_id,
            commands::update_profile,
            commands::login_cooldown,
            commands::logout,
            commands::session_activity,
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
            // Items (the generic surface, all kinds)
            commands::add_item,
            commands::update_item,
            commands::delete_item,
            commands::move_item,
            commands::get_item_by_id,
            commands::get_all_items,
            commands::get_item_secret,
            commands::copy_item_secret,
            commands::update_item_secret,
            commands::get_item_password_strength,
            // Util
            commands::get_password_strength,
            commands::flush_vault,
            commands::flush_all,
            commands::set_autosave,
            commands::is_vault_dirty
        ])
        .on_window_event(|_window, event| {
            // Best-effort: do not leave a copied secret behind on exit.
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                clipboard::clear();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
