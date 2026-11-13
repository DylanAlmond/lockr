use std::collections::HashMap;
use std::path::PathBuf;
use std::println;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use uuid::Uuid;
use zeroize::Zeroize;
use zxcvbn::{zxcvbn, Entropy};

use crate::mdns::{DiscoverySession, MdnsDiscovery};
use crate::network::{NetworkError, PeerNetwork};
use crate::sync::{
    PendingSync, SyncCredentials, SyncEngine, SyncOutcome, SyncProgress, SyncReport, SyncStateStore,
};
use crate::user_manager::UserManager;
use crate::vault_manager::VaultManager;
use crate::{
    DuplicateOutcome, DuplicateResolution, DuplicateSuggestion, IntoSafe, ItemFilter, NewItemData,
    PairingApprovalPrompt, PairingCode, PairingMessage, PairingOffer, SafeItem, SafeVault,
    SecretInput, User, UserId, VaultError, VaultId, CURRENT_SCHEMA_VERSION,
};

pub struct ManagerState {
    /// Where the managers read and write. Kept so an explicit identity
    /// recovery can re-initialise them without restarting the process.
    pub app_data_dir: PathBuf,

    /// `None` while a damaged Device identity is blocking startup. No command
    /// may touch vaults or profiles until the User chooses a recovery.
    pub vault_manager: Option<VaultManager>,
    pub user_manager: Option<UserManager>,

    /// Master password — only needed for create_vault / unlock_vault.
    /// Held in memory so the user can unlock additional vaults without
    /// re-entering their password. Zeroized on logout.
    pub master_password: Option<String>,
    pub secret_key: Option<Vec<u8>>,

    /// A distinct, user-visible startup failure. Today this is only a damaged
    /// Device identity file; it is surfaced instead of panicking the process.
    pub startup_error: Option<String>,

    /// Whether the failure is specifically a damaged Device identity, the one
    /// case where starting with a new identity is a sensible way out.
    pub device_identity_damaged: bool,

    /// The single network seam, kept in the state so discovery commands and
    /// the sync engine share one implementation.
    pub network: Option<Arc<MdnsDiscovery>>,

    /// The active discovery session. Dropping it stops advertising.
    pub discovery: Option<DiscoverySession>,

    /// Device-local Sync Set and last-sync timestamps. Never part of a Vault.
    pub sync_state: Option<SyncStateStore>,

    /// Conflicts holding a sync open, keyed by the report's session id.
    pub pending_syncs: HashMap<Uuid, PendingSync>,
}

impl ManagerState {
    /// Build the shared state. A damaged Device identity leaves the managers
    /// unset and records a recoverable error rather than aborting the app.
    pub fn initialize(app_data_dir: PathBuf) -> Self {
        match (
            VaultManager::new(app_data_dir.clone()),
            UserManager::new(app_data_dir.clone()),
        ) {
            (Ok(vault_manager), Ok(user_manager)) => {
                let network = Arc::new(MdnsDiscovery::new(vault_manager.device_id()));
                let sync_state = SyncStateStore::new(&app_data_dir).ok();
                Self {
                    app_data_dir,
                    vault_manager: Some(vault_manager),
                    user_manager: Some(user_manager),
                    master_password: None,
                    secret_key: None,
                    startup_error: None,
                    device_identity_damaged: false,
                    network: Some(network),
                    discovery: None,
                    sync_state,
                    pending_syncs: HashMap::new(),
                }
            }
            (vault_result, user_result) => {
                let device_identity_damaged = matches!(
                    &vault_result,
                    Err(crate::VaultError::DeviceIdentityDamaged(_))
                ) || matches!(
                    &user_result,
                    Err(crate::VaultError::DeviceIdentityDamaged(_))
                );

                let error = vault_result
                    .err()
                    .or_else(|| user_result.err())
                    .map(|err| err.to_string())
                    .unwrap_or_else(|| "Lockr could not initialize".to_string());

                Self {
                    app_data_dir,
                    vault_manager: None,
                    user_manager: None,
                    master_password: None,
                    secret_key: None,
                    startup_error: Some(error),
                    device_identity_damaged,
                    network: None,
                    discovery: None,
                    sync_state: None,
                    pending_syncs: HashMap::new(),
                }
            }
        }
    }

    fn startup_message(&self) -> String {
        self.startup_error
            .clone()
            .unwrap_or_else(|| "Lockr is not initialized".to_string())
    }

    /// Abandon a damaged Device identity and rebuild the managers under a
    /// freshly minted one. One-way by construction: the caller must already
    /// have obtained explicit confirmation.
    pub fn recover_with_new_device_identity(&mut self) -> Result<(), String> {
        if !self.device_identity_damaged {
            return Err("This Device's identity is not damaged".into());
        }

        let app_data_dir = self.app_data_dir.clone();
        crate::device::reset_device_id(&app_data_dir).map_err(|e| e.to_string())?;

        *self = ManagerState::initialize(app_data_dir);

        match &self.startup_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }

    fn ready(&self) -> Result<(&VaultManager, &UserManager), String> {
        match (&self.vault_manager, &self.user_manager) {
            (Some(vault_manager), Some(user_manager)) => Ok((vault_manager, user_manager)),
            _ => Err(self.startup_message()),
        }
    }

    fn ready_mut(&mut self) -> Result<(&mut VaultManager, &mut UserManager), String> {
        let message = self.startup_message();
        match (&mut self.vault_manager, &mut self.user_manager) {
            (Some(vault_manager), Some(user_manager)) => Ok((vault_manager, user_manager)),
            _ => Err(message),
        }
    }

    fn parts_mut(
        &mut self,
    ) -> Result<
        (
            &mut VaultManager,
            &mut UserManager,
            &mut Option<String>,
            &mut Option<Vec<u8>>,
        ),
        String,
    > {
        let message = self.startup_message();
        match (&mut self.vault_manager, &mut self.user_manager) {
            (Some(vault_manager), Some(user_manager)) => Ok((
                vault_manager,
                user_manager,
                &mut self.master_password,
                &mut self.secret_key,
            )),
            _ => Err(message),
        }
    }
}

type AppState<'a> = State<'a, Mutex<ManagerState>>;

#[tauri::command]
pub fn register_user(
    state: AppState,
    name: String,
    master_password: String,
) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    let user = user_manager
        .register(name, &master_password)
        .map_err(|e| e.to_string())?;

    let sk = user_manager
        .login(&user.id, &master_password)
        .map_err(|e| e.to_string())?;

    *mp_state = Some(master_password);
    *sk_state = Some(sk);

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    // Create default vault
    let new_vault = vault_manager
        .create_vault("Personal".to_string(), "#6240BF".to_string(), mp, sk)
        .map_err(|e| e.to_string())?;

    user_manager
        .add_vault_to_user(new_vault.id)
        .map_err(|e| e.to_string())?;

    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn get_users(state: AppState) -> Result<Vec<User>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    Ok(user_manager.get_users().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user(state: AppState) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let user_id = user_manager
        .active_user_id
        .as_ref()
        .ok_or_else(|| "No user is active".to_string())?;

    println!("id: {}", user_id);

    Ok(user_manager.get_user(user_id).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user_by_id(state: AppState, id: String) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let user_id: UserId = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    Ok(user_manager.get_user(&user_id).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn update_profile(
    state: AppState,
    name: Option<String>,
    color: Option<String>,
    icon: Option<String>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready_mut()?;
    user_manager
        .update_profile(name, color, icon)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn login_user(
    state: AppState,
    id: String,
    master_password: String,
) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    let user_id: UserId = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    let sk = user_manager
        .login(&user_id, &master_password)
        .map_err(|e| e.to_string())?;

    *mp_state = Some(master_password);
    *sk_state = Some(sk);

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    let user = user_manager.get_user(&user_id).map_err(|e| e.to_string())?;

    // Unlock all vaults — each derives its master key once (Argon2id).
    // After this, all further operations use the cached keys.
    for vault_id in &user.vault_ids {
        let _ = vault_manager.unlock_vault(*vault_id, mp, sk);
    }

    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn logout(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    user_manager.logout().map_err(|e| e.to_string())?;

    // Flush any unsaved changes before locking
    vault_manager.flush_all().map_err(|e| e.to_string())?;

    vault_manager.lock_vault(None);

    if let Some(mut mp) = mp_state.take() {
        mp.zeroize();
    }
    if let Some(mut sk) = sk_state.take() {
        sk.zeroize();
    }

    Ok(())
}

#[tauri::command]
pub fn delete_user(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    vault_manager.lock_vault(None);

    if let Some(mut mp) = mp_state.take() {
        mp.zeroize();
    }
    if let Some(mut sk) = sk_state.take() {
        sk.zeroize();
    }

    user_manager.delete_user().map_err(|e| e.to_string())
}

/// The distinct startup failure blocking normal use, if any. A damaged
/// Device identity file is reported here instead of panicking the process.
#[tauri::command]
pub fn get_device_identity_error(state: AppState) -> Result<Option<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state.startup_error.clone())
}

/// Explicitly start again as a new Device after a damaged identity file.
///
/// This is one-way: this installation abandons its old Device id and mints a
/// fresh one. The damaged file is kept beside the new one for inspection.
/// This path has no master password and no signing key, so it does **not**
/// revoke the old Device in the registry; another admitted Device must do
/// that. `confirm` exists so the action can never be taken by an accidental
/// click.
#[tauri::command]
pub fn start_with_new_device_identity(state: AppState, confirm: bool) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    if !state.device_identity_damaged {
        return Err("This Device's identity is not damaged".into());
    }
    if !confirm {
        return Err(format!(
            "{} Pass confirm=true to proceed.",
            crate::device::NEW_IDENTITY_EFFECT
        ));
    }

    state.recover_with_new_device_identity()
}

// ============ Pairing ============

/// Source Device: mint a single-use offer. The secret key is held by the
/// backend for the life of the offer and is never part of the offer itself.
#[tauri::command]
pub fn start_pairing(state: AppState) -> Result<PairingOffer, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager, _, secret_key) = state.parts_mut()?;

    let secret_key = secret_key.as_ref().ok_or("Not logged in")?;
    user_manager
        .start_pairing(secret_key, CURRENT_SCHEMA_VERSION, Utc::now())
        .map_err(|e| e.to_string())
}

/// Source Device: take the joining Device's announcement and return what the
/// human must approve. Nothing is admitted and no key material moves here.
#[tauri::command]
pub fn receive_pairing_request(
    state: AppState,
    message: PairingMessage,
) -> Result<PairingApprovalPrompt, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready_mut()?;

    user_manager
        .receive_pairing_request(message, Utc::now())
        .map_err(|e| e.to_string())
}

/// Source Device: approve the Device shown in the prompt. The returned frame
/// belongs on the authenticated pairing channel; it carries key material and
/// is never a view for display.
#[tauri::command]
pub fn approve_pairing(
    state: AppState,
    device_id: String,
    sync_set: Option<Vec<String>>,
) -> Result<PairingMessage, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready_mut()?;
    let device_id = Uuid::parse_str(&device_id).map_err(|e| e.to_string())?;

    // The pre-checked list is the joining Device's initial Sync Set. `None`
    // leaves the default: every Vault in the shared registry.
    let sync_set = sync_set
        .map(|ids| {
            ids.into_iter()
                .map(|id| Uuid::parse_str(&id).map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    user_manager
        .approve_pairing_with_sync_set(device_id, sync_set, Utc::now())
        .map_err(|e| e.to_string())
}

/// Abandon a live offer or an in-progress join.
#[tauri::command]
pub fn cancel_pairing(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready_mut()?;
    user_manager.cancel_pairing();
    Ok(())
}

/// Joining Device: announce this Device with the scanned or typed code. The
/// returned frame belongs on the pairing channel established with the code's
/// secret; nothing is written until the transfer arrives.
#[tauri::command]
pub fn join_pairing(state: AppState, code: PairingCode) -> Result<PairingMessage, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready_mut()?;

    user_manager
        .join_pairing(&code, CURRENT_SCHEMA_VERSION, Utc::now())
        .map_err(|e| e.to_string())
}

/// Joining Device: accept the source's transfer and re-wrap the secret key
/// under the password typed locally. The password never leaves this Device.
#[tauri::command]
pub fn complete_pairing(
    state: AppState,
    message: PairingMessage,
    master_password: String,
) -> Result<User, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager, master_password_state, secret_key_state) = state.parts_mut()?;

    // The source's pre-checked list becomes this Device's initial Sync Set.
    // Extract it before the transfer is consumed.
    let initial_sync_set = message
        .as_transfer()
        .and_then(|transfer| transfer.sync_set.clone());

    let user = user_manager
        .complete_pairing(message, &master_password, Utc::now())
        .map_err(|e| e.to_string())?;

    // A Device that has just paired is logged in on the strength of the
    // password typed here, exactly as if it had unlocked its own profile.
    let secret_key = user_manager
        .login(&user.id, &master_password)
        .map_err(|e| e.to_string())?;

    *master_password_state = Some(master_password);
    *secret_key_state = Some(secret_key);

    if let Some(ids) = initial_sync_set {
        let ids = ids
            .into_iter()
            .filter(|vault_id| user.vault_ids.contains(vault_id))
            .collect::<Vec<_>>();
        if let Some(store) = state.sync_state.as_mut() {
            store
                .set_sync_set(&user.id, ids)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(user)
}

#[tauri::command]
pub fn list_vault_ids(state: AppState) -> Result<Vec<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    vault_manager.list_vault_ids().map_err(|e| e.to_string())
}

/// Returns metadata for all vaults currently sitting in RAM
#[tauri::command]
pub fn get_unlocked_vaults(state: AppState) -> Result<Vec<SafeVault>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn create_vault(state: AppState, name: String, color: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    let (vault_manager, user_manager, mp_state, sk_state) = state.parts_mut()?;

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    let vault = vault_manager
        .create_vault(name, color, mp, sk)
        .map_err(|e| e.to_string())?;

    user_manager
        .add_vault_to_user(vault.id)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn get_vault_by_id(state: AppState, vault_id: String) -> Result<SafeVault, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let vault = vault_manager.get_vault(id).map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn unlock_vault(state: AppState, vault_id: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    // Vue sends a string but Rust needs a Uuid
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let (vault_manager, _, mp_state, sk_state) = state.parts_mut()?;

    let mp = mp_state.as_ref().ok_or("Not logged in")?;
    let sk = sk_state.as_ref().ok_or("Not logged in")?;

    let vault = vault_manager
        .unlock_vault(id, mp, sk)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn lock_vault(state: AppState, vault_id: Option<String>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let id = vault_id
        .map(|vid| Uuid::parse_str(&vid).map_err(|e| e.to_string()))
        .transpose()?;

    vault_manager.lock_vault(id);
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
    let (vault_manager, _) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    vault_manager
        .update_vault(id, name, color)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, user_manager) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    user_manager
        .remove_vault_from_user(id)
        .map_err(|e| e.to_string())?;

    vault_manager.delete_vault(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_password_strength(
    state: AppState,
    vault_id: String,
    item_id: String,
) -> Result<Entropy, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .get_item_password_strength(vid, iid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_password_strength(password: String) -> Result<Entropy, String> {
    Ok(zxcvbn(&password, &[]).into())
}

/// Flush a specific vault's unsaved changes to disk.
/// Call this after batch operations when autosave is off.
#[tauri::command]
pub fn flush_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    vault_manager.flush_vault(id).map_err(|e| e.to_string())
}

/// Flush ALL dirty vaults to disk. Call this before backgrounding,
/// navigating away, or periodically as a safety net.
#[tauri::command]
pub fn flush_all(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    vault_manager.flush_all().map_err(|e| e.to_string())
}

/// Toggle autosave on/off.
/// When off, mutations only mark vaults dirty — you must call
/// flush_vault / flush_all to persist.
#[tauri::command]
pub fn set_autosave(state: AppState, enabled: bool) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    vault_manager.set_autosave(enabled);
    Ok(())
}

/// Check if a vault has unsaved changes.
#[tauri::command]
pub fn is_vault_dirty(state: AppState, vault_id: String) -> Result<bool, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    Ok(vault_manager.is_dirty(id))
}

// ============ Items (the generic mutation/query surface, all kinds) ============

#[tauri::command]
pub fn add_item(
    state: AppState,
    vault_id: String,
    display_name: Option<String>,
    icon: Option<String>,
    data: NewItemData,
    secrets: Option<Vec<SecretInput>>,
) -> Result<SafeItem, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    vault_manager
        .add_item(id, display_name, icon, data, secrets)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_item(
    state: AppState,
    vault_id: String,
    item_id: String,
    display_name: Option<String>,
    icon: Option<String>,
    favourite: Option<bool>,
    tags: Option<Vec<String>>,
    color: Option<String>,
    data: Option<NewItemData>,
) -> Result<SafeItem, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .update_item(vid, iid, display_name, icon, favourite, tags, color, data)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_item(state: AppState, vault_id: String, item_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .delete_item(vid, iid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn move_item(
    state: AppState,
    vault_id: String,
    item_id: String,
    target_vault_id: String,
) -> Result<SafeItem, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;
    let tid = Uuid::parse_str(&target_vault_id).map_err(|e| e.to_string())?;

    vault_manager
        .move_item(vid, iid, tid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_by_id(state: AppState, item_id: String) -> Result<SafeItem, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let id = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager.get_item_by_id(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_all_items(state: AppState, filter: ItemFilter) -> Result<Vec<SafeItem>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;

    vault_manager
        .get_all_items(filter)
        .map_err(|e| e.to_string())
}

// ============ Per-field secrets ============

#[tauri::command]
pub fn get_item_secret(
    state: AppState,
    vault_id: String,
    item_id: String,
    field_key: String,
) -> Result<Option<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .get_item_secret(vid, iid, &field_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_item_secret(
    state: AppState,
    vault_id: String,
    item_id: String,
    field_key: String,
    value: Option<String>,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    vault_manager
        .update_item_secret(vid, iid, &field_key, value)
        .map_err(|e| e.to_string())
}

// ============ Duplicates ============

/// Suggest Duplicate pairs in one vault. Read-only: nothing is applied.
#[tauri::command]
pub fn get_duplicate_suggestions(
    state: AppState,
    vault_id: String,
) -> Result<Vec<DuplicateSuggestion>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    vault_manager
        .duplicate_suggestions(vid)
        .map_err(|e| e.to_string())
}

/// Resolve a suggested pair: keep both, keep either identity, or Combine
/// with an explicit choice for every colliding field.
#[tauri::command]
pub fn resolve_duplicate(
    state: AppState,
    vault_id: String,
    first: String,
    second: String,
    resolution: DuplicateResolution,
) -> Result<DuplicateOutcome, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, _) = state.ready_mut()?;
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let first = Uuid::parse_str(&first).map_err(|e| e.to_string())?;
    let second = Uuid::parse_str(&second).map_err(|e| e.to_string())?;

    vault_manager
        .resolve_duplicate(vid, first, second, resolution)
        .map_err(|e| e.to_string())
}

// ============ Devices, discovery and replication ============

/// A paired Device as the Devices screen needs it. No key material.
#[derive(Debug, Clone, Serialize)]
pub struct PairedDeviceView {
    pub device_id: crate::DeviceId,
    pub name: String,
    pub platform: String,
    pub admitted: bool,
    pub revoked: bool,
    pub last_sync_at: Option<DateTime<Utc>>,
    pub is_this_device: bool,
}

/// One peer seen by mDNS. The payload is deliberately limited to an opaque
/// device id, an address, and whether this User has already paired it.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredPeerView {
    pub device_id: crate::DeviceId,
    pub address: String,
    pub paired: bool,
}

/// One Vault as the Sync Set editor sees it.
#[derive(Debug, Clone, Serialize)]
pub struct SyncSetVaultView {
    pub vault_id: VaultId,
    pub name: Option<String>,
    pub syncing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncSetView {
    pub vaults: Vec<SyncSetVaultView>,
}

/// An unregistered `.vault` file found on disk, surfaced for adoption.
#[derive(Debug, Clone, Serialize)]
pub struct UnregisteredVaultView {
    pub vault_id: VaultId,
    pub file_name: String,
}

/// Which side of a Conflict the User picked. The backend holds both values,
/// so a secret never has to cross to the UI to be chosen.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictChoice {
    Local,
    Remote,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConflictResolutionResult {
    pub remaining: usize,
}

fn sync_discovery_error(err: NetworkError) -> String {
    match err {
        NetworkError::DiscoveryFailed(detail) => {
            VaultError::SyncDiscoveryFailed(detail).to_string()
        }
        NetworkError::TransportUnavailable(detail) => {
            VaultError::SyncTransportUnavailable(detail).to_string()
        }
        other => other.to_string(),
    }
}

/// Sync with one paired, discovered Device. Progress is emitted as
/// `sync-progress` Tauri events; the engine itself only knows a callback.
#[tauri::command]
pub fn sync_with_device(
    app: tauri::AppHandle,
    state: AppState,
    device_id: String,
) -> Result<SyncReport, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let peer_device_id: crate::DeviceId = Uuid::parse_str(&device_id).map_err(|e| e.to_string())?;

    let user_id = state
        .user_manager
        .as_ref()
        .and_then(|manager| manager.active_user_id)
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;

    if state.master_password.is_none() || state.secret_key.is_none() {
        return Err(VaultError::SyncNotLoggedIn.to_string());
    }

    let sync_set = state
        .sync_state
        .as_ref()
        .ok_or_else(|| {
            VaultError::SyncRefused("device-local sync state is unavailable".into()).to_string()
        })?
        .scope(&user_id);

    let network = state.network.clone().ok_or_else(|| {
        VaultError::SyncTransportUnavailable("no network seam".into()).to_string()
    })?;

    network.start_advertising().map_err(sync_discovery_error)?;

    let peer = match network.discover().map_err(sync_discovery_error) {
        Ok(peers) => peers
            .into_iter()
            .find(|peer| peer.device_id() == peer_device_id),
        Err(error) => {
            let _ = network.stop_advertising();
            return Err(error);
        }
    };

    let Some(peer) = peer else {
        let _ = network.stop_advertising();
        return Err(VaultError::SyncPeerUnreachable(peer_device_id.to_string()).to_string());
    };

    let result = (|| -> Result<SyncOutcome, VaultError> {
        // Borrow only the fields the engine needs; the network and state
        // store are used again after the sync ends.
        let ManagerState {
            vault_manager,
            user_manager,
            master_password,
            secret_key,
            ..
        } = &mut *state;

        let vault_manager = vault_manager
            .as_mut()
            .ok_or_else(|| VaultError::SyncRefused("vault manager is unavailable".into()))?;
        let user_manager = user_manager
            .as_ref()
            .ok_or_else(|| VaultError::SyncRefused("user manager is unavailable".into()))?;
        let master_password = master_password
            .as_deref()
            .ok_or(VaultError::SyncNotLoggedIn)?;
        let secret_key = secret_key.as_deref().ok_or(VaultError::SyncNotLoggedIn)?;

        let mut engine = SyncEngine::new(
            network.as_ref(),
            vault_manager,
            user_manager,
            sync_set,
            SyncCredentials {
                master_password,
                secret_key,
            },
        );

        let mut progress = |progress: SyncProgress| {
            let _ = app.emit("sync-progress", &progress);
        };

        let start = engine.start_with_peer(&peer, &mut progress)?;
        let sent = engine.send_offers(start, &mut progress)?;
        engine.collect(sent, &mut progress)
    })();

    let _ = network.stop_advertising();
    let outcome = result.map_err(|error| error.to_string())?;
    let SyncOutcome { report, pending } = outcome;

    if pending.is_empty() {
        if let Some(store) = state.sync_state.as_mut() {
            store
                .record_sync(&user_id, peer_device_id, Utc::now())
                .map_err(|error| error.to_string())?;
        }
    } else {
        state.pending_syncs.insert(
            report.session_id,
            PendingSync {
                user_id,
                conflicts: pending,
            },
        );
    }

    Ok(report)
}

/// Paired Devices, including this one, with last-sync state per peer.
#[tauri::command]
pub fn list_paired_devices(state: AppState) -> Result<Vec<PairedDeviceView>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let user_id = user_manager
        .active_user_id
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
    let shared = user_manager
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?;

    let mut devices = Vec::with_capacity(shared.devices.len());
    for (device_id, record) in shared.devices.iter() {
        devices.push(PairedDeviceView {
            device_id: *device_id,
            name: record.name.clone(),
            platform: record.platform.clone(),
            admitted: !record.is_revoked(),
            revoked: record.is_revoked(),
            last_sync_at: state
                .sync_state
                .as_ref()
                .and_then(|store| store.last_sync_at(&user_id, *device_id)),
            is_this_device: *device_id == user_manager.device_id(),
        });
    }

    devices.sort_by_key(|device| (device.revoked, device.name.clone(), device.device_id));
    Ok(devices)
}

/// Begin an mDNS discovery session. It is stopped explicitly or when the
/// process ends; an idle app must not be visible.
#[tauri::command]
pub fn start_discovery(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    if state.discovery.is_some() {
        return Ok(());
    }

    let network = state.network.clone().ok_or_else(|| {
        VaultError::SyncTransportUnavailable("no network seam".into()).to_string()
    })?;
    let session = DiscoverySession::start(&network).map_err(sync_discovery_error)?;
    state.discovery = Some(session);
    Ok(())
}

#[tauri::command]
pub fn stop_discovery(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    if let Some(session) = state.discovery.take() {
        session.finish();
    }
    Ok(())
}

/// Peers currently visible on the local network, marked paired or unknown.
/// The only fields are the opaque device id, its address, and the flag.
#[tauri::command]
pub fn list_discovered_peers(state: AppState) -> Result<Vec<DiscoveredPeerView>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let network = state.network.clone().ok_or_else(|| {
        VaultError::SyncTransportUnavailable("no network seam".into()).to_string()
    })?;

    let paired = state
        .user_manager
        .as_ref()
        .and_then(|manager| manager.active_user_id)
        .and_then(|user_id| {
            state
                .user_manager
                .as_ref()
                .and_then(|manager| manager.get_shared_profile(&user_id).ok())
        })
        .map(|shared| {
            shared
                .devices
                .iter()
                .map(|(device_id, _)| *device_id)
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();

    let peers = network.discover().map_err(sync_discovery_error)?;
    Ok(peers
        .into_iter()
        .map(|peer| DiscoveredPeerView {
            device_id: peer.device_id(),
            address: peer.address().to_string(),
            paired: paired.contains(&peer.device_id()),
        })
        .collect())
}

/// Read the Sync Set with per-Vault syncing state.
#[tauri::command]
pub fn get_sync_set(state: AppState) -> Result<SyncSetView, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, user_manager) = state.ready()?;
    let user_id = user_manager
        .active_user_id
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
    let shared = user_manager
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?;

    let sync_set = state
        .sync_state
        .as_ref()
        .map(|store| store.sync_set(&user_id, &shared.vaults))
        .unwrap_or_default();
    let names: HashMap<VaultId, String> = vault_manager
        .get_unlocked_vaults()
        .into_iter()
        .map(|vault| (vault.id, vault.name))
        .collect();

    let mut vaults: Vec<SyncSetVaultView> = shared
        .vaults
        .ids()
        .into_iter()
        .map(|vault_id| SyncSetVaultView {
            vault_id,
            name: names.get(&vault_id).cloned(),
            syncing: sync_set.contains(&vault_id),
        })
        .collect();
    vaults.sort_by_key(|vault| (vault.name.clone(), vault.vault_id));

    Ok(SyncSetView { vaults })
}

/// Replace the Sync Set with the User's explicit choice. Ids outside the
/// authoritative registry are refused.
#[tauri::command]
pub fn set_sync_set(state: AppState, vault_ids: Vec<String>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let user_id = user_manager
        .active_user_id
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
    let shared = user_manager
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?;

    let mut ids = Vec::with_capacity(vault_ids.len());
    for value in vault_ids {
        let vault_id: VaultId = Uuid::parse_str(&value).map_err(|e| e.to_string())?;
        if !shared.vaults.contains(&vault_id) {
            return Err(VaultError::SyncRefused(format!(
                "Vault {vault_id} is not in the shared Vault Registry"
            ))
            .to_string());
        }
        ids.push(vault_id);
    }

    state
        .sync_state
        .as_mut()
        .ok_or_else(|| VaultError::SyncRefused("sync state is unavailable".into()).to_string())?
        .set_sync_set(&user_id, ids)
        .map_err(|e| e.to_string())
}

/// Toggle one Vault in the Sync Set. Turning syncing off keeps the local copy
/// and its contents untouched.
#[tauri::command]
pub fn set_vault_syncing(state: AppState, vault_id: String, syncing: bool) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let user_id = user_manager
        .active_user_id
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
    let shared = user_manager
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?;
    let vault_id: VaultId = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    if !shared.vaults.contains(&vault_id) {
        return Err(VaultError::SyncRefused(format!(
            "Vault {vault_id} is not in the shared Vault Registry"
        ))
        .to_string());
    }

    state
        .sync_state
        .as_mut()
        .ok_or_else(|| VaultError::SyncRefused("sync state is unavailable".into()).to_string())?
        .set_vault_syncing(&user_id, vault_id, syncing, &shared.vaults)
        .map_err(|e| e.to_string())
}

/// Apply the User's explicit choice for one field or Vault-metadata Conflict.
/// Only this command writes a contested value; the sync itself left the local
/// value in place.
#[tauri::command]
pub fn resolve_sync_conflict(
    state: AppState,
    session_id: String,
    vault_id: String,
    item_id: Option<String>,
    field_key: String,
    choice: ConflictChoice,
) -> Result<ConflictResolutionResult, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let session_id: Uuid = Uuid::parse_str(&session_id).map_err(|e| e.to_string())?;
    let vault_id: VaultId = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let item_id: Option<crate::ItemId> = item_id
        .map(|value| Uuid::parse_str(&value).map_err(|e| e.to_string()))
        .transpose()?;

    let user_id = state
        .user_manager
        .as_ref()
        .and_then(|manager| manager.active_user_id)
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;

    let (scope, is_secret, value) = {
        let pending = state.pending_syncs.get(&session_id).ok_or_else(|| {
            VaultError::SyncConflictResolution("this sync session is no longer open".into())
                .to_string()
        })?;

        if pending.user_id != user_id {
            return Err(VaultError::SyncConflictResolution(
                "this sync session belongs to another User".into(),
            )
            .to_string());
        }

        let conflict = pending
            .conflicts
            .iter()
            .find(|conflict| {
                if conflict.vault_id != vault_id || conflict.field_key != field_key {
                    return false;
                }
                match (&conflict.scope, item_id) {
                    (crate::ConflictScope::Item { item_id: owned, .. }, Some(requested)) => {
                        *owned == requested
                    }
                    (crate::ConflictScope::Vault, None) => true,
                    _ => false,
                }
            })
            .ok_or_else(|| {
                VaultError::SyncConflictResolution(
                    "no open conflict matches that Vault and field".into(),
                )
                .to_string()
            })?;

        (
            conflict.scope.clone(),
            conflict.is_secret,
            match choice {
                ConflictChoice::Local => conflict.local.clone(),
                ConflictChoice::Remote => conflict.remote.clone(),
            },
        )
    };

    let applied = state
        .vault_manager
        .as_mut()
        .ok_or_else(|| VaultError::SyncRefused("vault manager is unavailable".into()).to_string())?
        .resolve_conflict_field(vault_id, &scope, &field_key, is_secret, value.as_ref())
        .map_err(|e| e.to_string())?;

    if !applied {
        return Err(VaultError::SyncConflictResolution(
            "the contested field no longer exists".into(),
        )
        .to_string());
    }

    let pending = state
        .pending_syncs
        .get_mut(&session_id)
        .expect("checked above");
    pending.conflicts.retain(|conflict| {
        !(conflict.vault_id == vault_id
            && conflict.field_key == field_key
            && match (&conflict.scope, item_id) {
                (crate::ConflictScope::Item { item_id: owned, .. }, Some(requested)) => {
                    *owned == requested
                }
                (crate::ConflictScope::Vault, None) => true,
                _ => false,
            })
    });
    let remaining = pending.conflicts.len();
    if remaining == 0 {
        state.pending_syncs.remove(&session_id);
    }

    Ok(ConflictResolutionResult { remaining })
}

/// Local `.vault` files absent from the shared Vault Registry. They are never
/// auto-adopted: adoption shares the Vault's entire contents.
#[tauri::command]
pub fn list_unregistered_vaults(state: AppState) -> Result<Vec<UnregisteredVaultView>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, user_manager) = state.ready()?;
    let user_id = user_manager
        .active_user_id
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
    let shared = user_manager
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?;

    Ok(vault_manager
        .unregistered_vault_files(&shared.vaults)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|vault_id| UnregisteredVaultView {
            vault_id,
            file_name: format!("{vault_id}.vault"),
        })
        .collect())
}

/// Explicitly adopt an unregistered local Vault file. Adoption inserts it
/// into the authoritative registry *and* this Device's Sync Set, because
/// adopting it shares it with the other Devices.
#[tauri::command]
pub fn adopt_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, user_manager) = state.ready_mut()?;
    let user_id = user_manager
        .active_user_id
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
    let vault_id: VaultId = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let shared = user_manager
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?;
    if shared.vaults.contains(&vault_id) {
        return Err(
            VaultError::SyncRefused(format!("Vault {vault_id} is already registered")).to_string(),
        );
    }
    if !vault_manager.has_vault_file(vault_id) {
        return Err(VaultError::NotFound(vault_id.to_string()).to_string());
    }

    user_manager
        .add_vault_to_user(vault_id)
        .map_err(|e| e.to_string())?;

    let registry = user_manager
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?
        .vaults;

    state
        .sync_state
        .as_mut()
        .ok_or_else(|| VaultError::SyncRefused("sync state is unavailable".into()).to_string())?
        .adopt(&user_id, vault_id, &registry)
        .map_err(|e| e.to_string())
}

/// Revoke a lost Device with the existing signed registry operation. The
/// revocation travels on the next sync with every other Device.
#[tauri::command]
pub fn revoke_device(state: AppState, device_id: String) -> Result<(), String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (_, user_manager) = state.ready()?;
    let device_id: crate::DeviceId = Uuid::parse_str(&device_id).map_err(|e| e.to_string())?;

    user_manager
        .revoke_device(device_id, Utc::now())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> PathBuf {
        let temp_dir = std::env::temp_dir().join(format!("lockr_commands_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_a_damaged_device_identity_is_surfaced_without_panicking_or_being_replaced() {
        let temp_dir = setup();
        std::fs::write(temp_dir.join("device_id"), b"not-a-uuid").unwrap();

        let mut state = ManagerState::initialize(temp_dir.clone());

        let error = state
            .startup_error
            .as_ref()
            .expect("a damaged identity must be reported");
        assert!(
            error.contains("Device identity"),
            "unexpected error: {error}"
        );
        assert!(state.device_identity_damaged);
        assert!(state.vault_manager.is_none());
        assert!(state.user_manager.is_none());
        assert!(state.ready().is_err());
        assert_eq!(
            std::fs::read(temp_dir.join("device_id")).unwrap(),
            b"not-a-uuid",
            "startup must not silently replace the identity"
        );

        // Recovery is explicit and one-way, and puts the app back to work.
        state.recover_with_new_device_identity().unwrap();
        assert!(state.startup_error.is_none());
        assert!(state.ready().is_ok());
        assert_ne!(
            std::fs::read(temp_dir.join("device_id")).unwrap(),
            b"not-a-uuid"
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_an_unreadable_device_identity_offers_the_same_recovery_path() {
        let temp_dir = setup();
        std::fs::create_dir(temp_dir.join("device_id")).unwrap();

        let mut state = ManagerState::initialize(temp_dir.clone());

        assert!(
            state.device_identity_damaged,
            "an unreadable identity must be offered the new-identity recovery"
        );
        assert!(state.startup_error.is_some());
        assert!(state.ready().is_err());

        state.recover_with_new_device_identity().unwrap();
        assert!(state.ready().is_ok());
        assert!(!temp_dir.join("device_id").is_dir());

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_identity_recovery_is_refused_when_there_is_nothing_to_recover() {
        let temp_dir = setup();
        let mut state = ManagerState::initialize(temp_dir.clone());

        let result = state.recover_with_new_device_identity();

        assert!(result.is_err());
        std::fs::remove_dir_all(temp_dir).ok();
    }
}
