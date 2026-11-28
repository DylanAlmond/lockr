fn main() {
    // Declaring the app commands generates `allow-<command>` / `deny-<command>`
    // permissions and puts app commands under the ACL: a window can only call
    // the commands its capability grants.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                // Session and user
                "get_users",
                "register_user",
                "login_user",
                "get_user",
                "get_user_by_id",
                "update_profile",
                "login_cooldown",
                "logout",
                "session_activity",
                "delete_user",
                // Vaults
                "list_vault_ids",
                "get_unlocked_vaults",
                "get_vault_by_id",
                "create_vault",
                "unlock_vault",
                "lock_vault",
                "update_vault",
                "delete_vault",
                // Items
                "add_item",
                "update_item",
                "delete_item",
                "move_item",
                "get_item_by_id",
                "get_all_items",
                "get_item_secret",
                "copy_item_secret",
                "update_item_secret",
                "get_item_password_strength",
                // Utility
                "get_password_strength",
                "flush_vault",
                "flush_all",
                "set_autosave",
                "is_vault_dirty",
            ]),
        ),
    )
    .expect("failed to run tauri-build");
}
