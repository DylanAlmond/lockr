mod atomic;
mod commands;
mod crypto;
mod device;
mod duplicates;
mod error;
mod mdns;
mod merge;
mod models;
mod network;
mod pairing;
mod registry;
mod sync;
mod transport;
mod user_manager;
mod vault_manager;

use std::sync::Mutex;

pub use duplicates::*;
pub use error::VaultError;
pub use mdns::{DiscoverySession, MdnsDiscovery};
pub use merge::*;
pub use models::*;
pub use network::*;
pub use pairing::*;
pub use registry::*;
pub use sync::*;
pub use transport::{TlsNetwork, TlsTransportConfig};
use tauri::Manager;

use crate::commands::ManagerState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to resolve app data dir");

            // A damaged Device identity file must not panic the process. The
            // state records a distinct error that the UI can surface, and
            // `start_with_new_device_identity` is the explicit way out.
            let app_state = ManagerState::initialize(app_data_dir);

            app.manage(Mutex::new(app_state));

            // Sync control lives outside the global state so cancelling a sync
            // (and refusing a second one) never waits for the exchange's lock.
            app.manage(sync::SyncControl::default());

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            // User
            commands::get_users,
            commands::register_user,
            commands::login_user,
            commands::get_user,
            commands::get_user_by_id,
            commands::update_profile,
            commands::logout,
            commands::delete_user,
            commands::get_device_identity_error,
            commands::start_with_new_device_identity,
            // Pairing
            commands::start_pairing,
            commands::receive_pairing_request,
            commands::approve_pairing,
            commands::cancel_pairing,
            commands::join_pairing,
            commands::complete_pairing,
            // Devices and sync
            commands::sync_with_device,
            commands::cancel_sync,
            commands::list_paired_devices,
            commands::start_discovery,
            commands::stop_discovery,
            commands::list_discovered_peers,
            commands::get_sync_set,
            commands::set_sync_set,
            commands::set_vault_syncing,
            commands::resolve_sync_conflict,
            commands::list_unregistered_vaults,
            commands::adopt_vault,
            commands::revoke_device,
            // Vaults
            commands::list_vault_ids,
            commands::get_unlocked_vaults,
            commands::get_vault_by_id,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::update_vault,
            commands::delete_vault,
            commands::remove_local_vault,
            // Items (the generic surface, all kinds)
            commands::add_item,
            commands::update_item,
            commands::delete_item,
            commands::move_item,
            commands::get_item_by_id,
            commands::get_all_items,
            commands::get_item_secret,
            commands::update_item_secret,
            commands::get_item_password_strength,
            // Duplicates
            commands::get_duplicate_suggestions,
            commands::resolve_duplicate,
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
