use std::{
    cell::Cell,
    println,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};
use zxcvbn::{zxcvbn, Entropy};

use crate::user_manager::{validate_master_password, UserManager};
use crate::vault_manager::VaultManager;
use crate::{
    ItemFilter, ItemId, IntoSafe, NewItemData, SafeItem, SafeVault, SecretInput, User, UserId,
    VaultError, VaultId,
};

pub struct ManagerState {
    pub vault_manager: VaultManager,
    pub user_manager: UserManager,

    /// Master password — only needed for create_vault / unlock_vault.
    /// Held in memory so the user can unlock additional vaults without
    /// re-entering their password. Zeroized on logout.
    pub master_password: Option<String>,
    pub secret_key: Option<Vec<u8>>,

    /// When the session began and when it was last active. Used by the
    /// background monitor to enforce the idle and absolute timeouts.
    session_started: Cell<Option<Instant>>,
    last_activity: Cell<Option<Instant>>,
}

type AppState<'a> = State<'a, Mutex<ManagerState>>;

/// Defaults for the vault created with a new profile.
const DEFAULT_VAULT_NAME: &str = "Personal";
const DEFAULT_VAULT_COLOUR: &str = "#6240BF";

/// Session policy (docs/security/auth-policy.md).
const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const ABSOLUTE_TIMEOUT: Duration = Duration::from_secs(12 * 60 * 60);
const SESSION_CHECK_INTERVAL: Duration = Duration::from_secs(10);

/// Parse the vault/item id pair shared by the item commands.
fn parse_vault_item(vault_id: &str, item_id: &str) -> Result<(VaultId, ItemId), String> {
    Ok((
        Uuid::parse_str(vault_id).map_err(|e| e.to_string())?,
        Uuid::parse_str(item_id).map_err(|e| e.to_string())?,
    ))
}

impl ManagerState {
    /// A logged-out state with both managers and no session.
    pub fn new(vault_manager: VaultManager, user_manager: UserManager) -> Self {
        Self {
            vault_manager,
            user_manager,
            master_password: None,
            secret_key: None,
            session_started: Cell::new(None),
            last_activity: Cell::new(None),
        }
    }

    /// Delete a vault the active user owns, after re-checking the master
    /// password.
    ///
    /// A compromised renderer must not be able to destroy vaults with a
    /// single IPC call, so deletion is gated on ownership and on a fresh
    /// password verification rather than on the cached session alone.
    pub fn delete_vault(
        &mut self,
        vault_id: VaultId,
        master_password: &str,
    ) -> Result<(), VaultError> {
        self.require_owned_vault(vault_id)?;
        self.user_manager.verify_password(master_password)?;

        // Move the file aside first, then record the deletion. If recording
        // fails, put the file back rather than leaving an orphaned tombstone
        // the user's profile still points at.
        let tombstone = self.vault_manager.delete_vault(vault_id)?;
        if let Err(error) = self.user_manager.remove_vault_from_user(vault_id) {
            let _ = self.vault_manager.restore_vault(vault_id, &tombstone);
            return Err(error);
        }

        Ok(())
    }

    /// End the current session: persist unsaved changes, then tear down. The
    /// teardown runs even when the flush fails, so a disk error cannot leave
    /// vaults unlocked or credentials cached.
    fn end_session(&mut self) -> Result<(), VaultError> {
        // Persist every unlocked vault, not only the active user's: the
        // session is ending and all RAM state must reach disk first.
        let flush = self.vault_manager.flush_all();
        self.teardown_session();
        flush
    }

    /// Drop the secrets and clocks of the session, leaving the active-user
    /// pointer alone. `delete_user` needs that pointer to remove the profile.
    fn clear_session_state(&mut self) {
        self.vault_manager.lock_vault(None);
        self.clear_credentials();
        // Do not leave a copied secret on the clipboard after logout.
        crate::clipboard::clear();
        self.session_started.set(None);
        self.last_activity.set(None);
    }

    /// Drop everything that belongs to the session. Infallible so a timeout
    /// can always end the session, even when the disk is unavailable.
    fn teardown_session(&mut self) {
        self.clear_session_state();
        self.user_manager.clear_active_user();
    }

    fn start_session(&self) {
        let now = Instant::now();
        self.session_started.set(Some(now));
        self.last_activity.set(Some(now));
    }

    /// Record activity for the idle timeout. No-op when no session is open.
    fn touch(&self) {
        if self.session_started.get().is_some() {
            self.last_activity.set(Some(Instant::now()));
        }
    }

    fn session_timed_out(&self) -> bool {
        let Some(started) = self.session_started.get() else {
            return false;
        };
        let last = self.last_activity.get().unwrap_or(started);
        let now = Instant::now();

        now.saturating_duration_since(last) >= IDLE_TIMEOUT
            || now.saturating_duration_since(started) >= ABSOLUTE_TIMEOUT
    }

    /// End the session if the idle or absolute timeout has passed. Returns
    /// whether it ended one. Flushing is best-effort: a timeout must not be
    /// blocked by a disk error.
    fn expire_session(&mut self) -> bool {
        if !self.session_timed_out() {
            return false;
        }

        // The flush is best-effort; teardown still runs on error.
        let _ = self.end_session();
        true
    }

    fn clear_credentials(&mut self) {
        if let Some(mut master_password) = self.master_password.take() {
            master_password.zeroize();
        }
        if let Some(mut secret_key) = self.secret_key.take() {
            secret_key.zeroize();
        }
    }

    /// Register a user and start their session. Any previous session ends
    /// first, so another user's unlocked vaults cannot outlive the switch.
    pub fn register_user(
        &mut self,
        name: String,
        master_password: String,
    ) -> Result<Vec<SafeVault>, VaultError> {
        // Reject a weak master password before ending any current session.
        validate_master_password(&name, &master_password)?;

        self.end_session()?;

        let user = self.user_manager.register(name, &master_password)?;

        // `register` encrypts a fresh secret key; `login` decrypts it so the
        // session holds the plaintext key. Two Argon2 passes, unavoidable.
        let secret_key = self.user_manager.login(&user.id, &master_password)?;

        let new_vault = match self.vault_manager.create_vault(
            DEFAULT_VAULT_NAME.to_string(),
            DEFAULT_VAULT_COLOUR.to_string(),
            &master_password,
            &secret_key,
        ) {
            Ok(vault) => vault,
            Err(error) => {
                // Do not leave an active user with no cached credentials.
                self.user_manager.clear_active_user();
                return Err(error);
            }
        };

        self.master_password = Some(master_password);
        self.secret_key = Some(secret_key);
        self.start_session();

        // If recording the vault fails, drop the half-started session rather
        // than leaving credentials cached with an unbounded clock.
        if let Err(error) = self.user_manager.add_vault_to_user(new_vault.id) {
            self.teardown_session();
            return Err(error);
        }

        Ok(vec![new_vault.into_safe()])
    }

    /// Log in as a user and unlock their vaults. Any previous session ends
    /// first, so another user's unlocked vaults cannot outlive the switch.
    pub fn login_user(
        &mut self,
        user_id: UserId,
        master_password: String,
    ) -> Result<Vec<SafeVault>, VaultError> {
        self.end_session()?;

        let secret_key = self.user_manager.login(&user_id, &master_password)?;
        let user = self.user_manager.get_user(&user_id)?;

        // Each vault derives its master key once (Argon2id); further
        // operations use the cached keys. A vault that fails to unlock stays
        // locked and the session continues with the rest.
        for vault_id in &user.vault_ids {
            let _ = self
                .vault_manager
                .unlock_vault(*vault_id, &master_password, &secret_key);
        }

        self.master_password = Some(master_password);
        self.secret_key = Some(secret_key);
        self.start_session();

        self.get_unlocked_vaults()
    }

    /// The vault ids the active user owns, from the profile's `vault_ids`.
    fn owned_vault_ids(&self) -> Result<Vec<VaultId>, VaultError> {
        self.touch();
        Ok(self.user_manager.active_user()?.vault_ids)
    }

    /// Reject a vault the active user does not own. Commands that read or
    /// mutate vault data call this first; locking and creating a vault do not
    /// need it.
    fn require_owned_vault(&self, vault_id: VaultId) -> Result<(), VaultError> {
        self.touch();

        if self.user_manager.owns_vault(vault_id)? {
            Ok(())
        } else {
            Err(VaultError::Unauthorised)
        }
    }

    /// Vault ids on disk that the active user owns.
    pub fn list_vault_ids(&self) -> Result<Vec<String>, VaultError> {
        let owned = self.owned_vault_ids()?;
        Ok(self
            .vault_manager
            .list_vault_ids()?
            .into_iter()
            .filter(|id| owned.contains(id))
            .map(|id| id.to_string())
            .collect())
    }

    /// Unlocked vaults the active user owns.
    pub fn get_unlocked_vaults(&self) -> Result<Vec<SafeVault>, VaultError> {
        let owned = self.owned_vault_ids()?;
        let mut vaults = self.vault_manager.get_unlocked_vaults();
        vaults.retain(|vault| owned.contains(&vault.id));
        Ok(vaults)
    }

    /// Items in the active user's vaults. A filter naming a vault outside
    /// that set is rejected; an unfiltered scan only visits owned vaults.
    pub fn get_all_items(&self, filter: ItemFilter) -> Result<Vec<SafeItem>, VaultError> {
        let owned = self.owned_vault_ids()?;

        if let Some(vault_id) = filter.vault_id {
            if !owned.contains(&vault_id) {
                return Err(VaultError::Unauthorised);
            }
        }

        self.vault_manager.get_all_items_in(&owned, filter)
    }

    /// An item in one of the active user's vaults, by id. An item in an
    /// unowned vault is not found rather than forbidden, so the response does
    /// not reveal that it exists.
    pub fn get_item_by_id(&self, item_id: ItemId) -> Result<SafeItem, VaultError> {
        let owned = self.owned_vault_ids()?;
        self.vault_manager.get_item_by_id_in(&owned, item_id)
    }

    /// The secret value a copy command needs, after ownership and field
    /// validation. Kept out of the command so the checks are testable without
    /// touching the OS clipboard.
    fn item_secret_for_copy(
        &self,
        vault_id: VaultId,
        item_id: ItemId,
        field_key: &str,
    ) -> Result<String, VaultError> {
        self.require_owned_vault(vault_id)?;

        self.vault_manager
            .get_item_secret(vault_id, item_id, field_key)?
            .ok_or_else(|| VaultError::Validation("This field has no value".to_string()))
    }

    /// Flush unsaved changes for the active user's vaults only.
    pub fn flush_all(&mut self) -> Result<(), VaultError> {
        for vault_id in self.owned_vault_ids()? {
            self.vault_manager.flush_vault(vault_id)?;
        }

        Ok(())
    }
}

#[tauri::command]
pub fn register_user(
    state: AppState,
    name: String,
    master_password: String,
) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    state
        .register_user(name, master_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_users(state: AppState) -> Result<Vec<User>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    Ok(state.user_manager.get_users().map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user(state: AppState) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let user_id = state
        .user_manager
        .active_user_id
        .as_ref()
        .ok_or_else(|| "No user is active".to_string())?;

    println!("id: {}", user_id);

    Ok(state
        .user_manager
        .get_user(user_id)
        .map_err(|e| e.to_string())?)
}

#[tauri::command]
pub fn get_user_by_id(state: AppState, id: String) -> Result<User, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let user_id: UserId = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    Ok(state
        .user_manager
        .get_user(&user_id)
        .map_err(|e| e.to_string())?)
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
pub fn login_user(
    state: AppState,
    id: String,
    master_password: String,
) -> Result<Vec<SafeVault>, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let user_id: UserId = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    state
        .login_user(user_id, master_password)
        .map_err(|e| e.to_string())
}

/// Seconds until a blocked user may try again. The renderer uses this to run
/// a live countdown; the core enforces the same timestamp.
#[tauri::command]
pub fn login_cooldown(state: AppState, id: String) -> Result<Option<u64>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let user_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

    Ok(state.user_manager.cooldown_remaining(&user_id))
}

#[tauri::command]
pub fn logout(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    state.end_session().map_err(|e| e.to_string())
}

/// Heartbeat for renderer-reported user input. It can only extend an existing
/// session; it never creates one.
#[tauri::command]
pub fn session_activity(state: AppState) -> Result<(), String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    state.touch();
    Ok(())
}

/// Poll the session clock and end the session after the idle or absolute
/// timeout, notifying the renderer so it can return to the auth screen.
pub fn spawn_session_monitor(handle: AppHandle) {
    thread::spawn(move || loop {
        thread::sleep(SESSION_CHECK_INTERVAL);

        let expired = {
            let app_state = handle.state::<Mutex<ManagerState>>();
            let mut state = match app_state.lock() {
                Ok(state) => state,
                // A panic elsewhere must not silently disable the timeout.
                Err(poisoned) => poisoned.into_inner(),
            };
            state.expire_session()
        };

        if expired {
            let _ = handle.emit("session-expired", ());
        }
    });
}

#[tauri::command]
pub fn delete_user(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    // `delete_user` removes the profile and clears the active user itself;
    // the account is going away, so there is no session to flush first.
    state.clear_session_state();

    state.user_manager.delete_user().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_vault_ids(state: AppState) -> Result<Vec<String>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    state.list_vault_ids().map_err(|e| e.to_string())
}

/// Returns metadata for the active user's vaults currently sitting in RAM
#[tauri::command]
pub fn get_unlocked_vaults(state: AppState) -> Result<Vec<SafeVault>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    state.get_unlocked_vaults().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_vault(state: AppState, name: String, color: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    state.touch();

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        user_manager,
        ..
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state.require_owned_vault(id).map_err(|e| e.to_string())?;

    let vault = state
        .vault_manager
        .get_vault(id)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn unlock_vault(state: AppState, vault_id: String) -> Result<SafeVault, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    // Vue sends a string but Rust needs a Uuid
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state.require_owned_vault(id).map_err(|e| e.to_string())?;

    let ManagerState {
        vault_manager,
        master_password,
        secret_key,
        ..
    } = &mut *state;

    let mp = master_password.as_ref().ok_or("Not logged in")?;
    let sk = secret_key.as_ref().ok_or("Not logged in")?;

    let vault = vault_manager
        .unlock_vault(id, mp, sk)
        .map_err(|e| e.to_string())?;

    Ok(vault.into_safe())
}

#[tauri::command]
pub fn lock_vault(state: AppState, vault_id: Option<String>) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = vault_id
        .map(|vid| Uuid::parse_str(&vid).map_err(|e| e.to_string()))
        .transpose()?;

    if let Some(id) = id {
        state.require_owned_vault(id).map_err(|e| e.to_string())?;
    }

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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state.require_owned_vault(id).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .update_vault(id, name, color)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_vault(
    state: AppState,
    vault_id: String,
    master_password: String,
) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state
        .delete_vault(id, &master_password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_password_strength(
    state: AppState,
    vault_id: String,
    item_id: String,
) -> Result<Entropy, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let (vid, iid) = parse_vault_item(&vault_id, &item_id)?;

    state.require_owned_vault(vid).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    state.require_owned_vault(id).map_err(|e| e.to_string())?;
    state
        .vault_manager
        .flush_vault(id)
        .map_err(|e| e.to_string())
}

/// Flush ALL dirty vaults to disk. Call this before backgrounding,
/// navigating away, or periodically as a safety net.
#[tauri::command]
pub fn flush_all(state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.flush_all().map_err(|e| e.to_string())
}

/// Toggle autosave on/off.
/// When off, mutations only mark vaults dirty — you must call
/// flush_vault / flush_all to persist.
#[tauri::command]
pub fn set_autosave(state: AppState, enabled: bool) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.vault_manager.set_autosave(enabled);
    Ok(())
}

/// Check if a vault has unsaved changes.
#[tauri::command]
pub fn is_vault_dirty(state: AppState, vault_id: String) -> Result<bool, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    state.require_owned_vault(id).map_err(|e| e.to_string())?;
    Ok(state.vault_manager.is_dirty(id))
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
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    state.require_owned_vault(id).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let (vid, iid) = parse_vault_item(&vault_id, &item_id)?;

    state.require_owned_vault(vid).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .update_item(vid, iid, display_name, icon, favourite, tags, color, data)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_item(state: AppState, vault_id: String, item_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vid, iid) = parse_vault_item(&vault_id, &item_id)?;

    state.require_owned_vault(vid).map_err(|e| e.to_string())?;

    state
        .vault_manager
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
    let vid = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;
    let iid = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;
    let tid = Uuid::parse_str(&target_vault_id).map_err(|e| e.to_string())?;

    state.require_owned_vault(vid).map_err(|e| e.to_string())?;
    state.require_owned_vault(tid).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .move_item(vid, iid, tid)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_item_by_id(state: AppState, item_id: String) -> Result<SafeItem, String> {
    let state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&item_id).map_err(|e| e.to_string())?;

    state
        .get_item_by_id(id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_all_items(state: AppState, filter: ItemFilter) -> Result<Vec<SafeItem>, String> {
    let state = state.lock().map_err(|e| e.to_string())?;

    state
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
    let (vid, iid) = parse_vault_item(&vault_id, &item_id)?;

    state.require_owned_vault(vid).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .get_item_secret(vid, iid, &field_key)
        .map_err(|e| e.to_string())
}

/// Copy a secret field to the OS clipboard without returning it to the
/// renderer. The clipboard is cleared after 30 seconds if it still holds the
/// copied value (secret-lifetime R-S4).
#[tauri::command]
pub fn copy_item_secret(
    state: AppState,
    vault_id: String,
    item_id: String,
    field_key: String,
) -> Result<(), String> {
    // Fetch under the state lock, then release it before the clipboard work
    // so a slow clipboard does not stall every other command.
    let value = {
        let state = state.lock().map_err(|e| e.to_string())?;
        let (vid, iid) = parse_vault_item(&vault_id, &item_id)?;
        let value = state
            .item_secret_for_copy(vid, iid, &field_key)
            .map_err(|e| e.to_string())?;

        Zeroizing::new(value)
    };

    crate::clipboard::write_secret_and_arm_clear(value.as_str())
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
    let (vid, iid) = parse_vault_item(&vault_id, &item_id)?;

    state.require_owned_vault(vid).map_err(|e| e.to_string())?;

    state
        .vault_manager
        .update_item_secret(vid, iid, &field_key, value)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const MASTER_PW: &str = "CorrectHorseBattery";
    const SECRET_KEY: [u8; 32] = [7u8; 32];

    /// A logged-in user with one owned vault and the raw managers, so the
    /// deletion preconditions can be exercised without a Tauri harness.
    fn setup() -> (ManagerState, VaultId, PathBuf) {
        let temp_dir = std::env::temp_dir().join(format!("commands_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();

        let mut user_manager = UserManager::new(temp_dir.clone()).unwrap();
        user_manager
            .register("Alice".to_string(), MASTER_PW)
            .unwrap();

        let mut vault_manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = vault_manager
            .create_vault(
                "Personal".to_string(),
                "#6240BF".to_string(),
                MASTER_PW,
                &SECRET_KEY,
            )
            .unwrap();
        user_manager.add_vault_to_user(vault.id).unwrap();

        let state = ManagerState {
            vault_manager,
            user_manager,
            master_password: Some(MASTER_PW.to_string()),
            secret_key: Some(SECRET_KEY.to_vec()),
            session_started: Cell::new(Some(Instant::now())),
            last_activity: Cell::new(Some(Instant::now())),
        };

        (state, vault.id, temp_dir)
    }

    #[test]
    fn test_delete_vault_requires_the_master_password() {
        let (mut state, vault_id, temp_dir) = setup();

        let result = state.delete_vault(vault_id, "WrongPassword");

        assert!(
            matches!(result, Err(VaultError::AuthenticationFailed)),
            "got {result:?}"
        );
        assert!(
            temp_dir
                .join("vaults")
                .join(format!("{}.vault", vault_id))
                .exists(),
            "a failed re-authentication must leave the vault in place"
        );
        assert!(
            state.user_manager.owns_vault(vault_id).unwrap(),
            "a failed re-authentication must not touch the user's vault list"
        );
    }

    #[test]
    fn test_delete_vault_rejects_a_vault_the_active_user_does_not_own() {
        let (mut state, _, temp_dir) = setup();
        let foreign = add_unowned_unlocked_vault(&mut state);

        let result = state.delete_vault(foreign, MASTER_PW);

        assert!(
            matches!(result, Err(VaultError::Unauthorised)),
            "got {result:?}"
        );
        assert!(
            temp_dir
                .join("vaults")
                .join(format!("{}.vault", foreign))
                .exists(),
            "an unowned vault must not be deleted"
        );
    }

    #[test]
    fn test_delete_vault_tombstones_an_owned_vault() {
        let (mut state, vault_id, temp_dir) = setup();

        state.delete_vault(vault_id, MASTER_PW).unwrap();

        let vaults_dir = temp_dir.join("vaults");
        assert!(
            !vaults_dir.join(format!("{}.vault", vault_id)).exists(),
            "the live vault file must be moved aside"
        );
        assert!(
            vaults_dir
                .join(format!("{}.vault.deleted", vault_id))
                .exists(),
            "the deleted vault must be recoverable"
        );
        assert!(
            !state.user_manager.owns_vault(vault_id).unwrap(),
            "deletion must remove the vault from the user's list"
        );
    }

    // ============ Ownership across the read surface ============

    fn add_account(state: &mut ManagerState, vault_id: VaultId, username: &str) -> Uuid {
        match state
            .vault_manager
            .add_item(
                vault_id,
                None,
                None,
                NewItemData::Account(crate::AccountData {
                    username: username.to_string(),
                    email: None,
                }),
                None,
            )
            .unwrap()
        {
            SafeItem::Account(account) => account.id,
            other => panic!("expected account item, got {other:?}"),
        }
    }

    /// A vault that is unlocked in RAM but missing from the active user's
    /// `vault_ids` — the state F-13 can leave behind.
    fn add_unowned_unlocked_vault(state: &mut ManagerState) -> VaultId {
        state
            .vault_manager
            .create_vault(
                "Foreign".to_string(),
                "#000000".to_string(),
                MASTER_PW,
                &SECRET_KEY,
            )
            .unwrap()
            .id
    }

    #[test]
    fn test_require_owned_vault_rejects_an_unowned_vault_id() {
        let (state, owned, _) = setup();

        state.require_owned_vault(owned).unwrap();

        let result = state.require_owned_vault(Uuid::new_v4());
        assert!(
            matches!(result, Err(VaultError::Unauthorised)),
            "got {result:?}"
        );
    }

    #[test]
    fn test_require_owned_vault_needs_an_active_user() {
        let (mut state, owned, _) = setup();
        state.user_manager.clear_active_user();

        let result = state.require_owned_vault(owned);
        assert!(
            matches!(result, Err(VaultError::UserNotFound(_))),
            "got {result:?}"
        );
    }

    #[test]
    fn test_item_secret_for_copy_returns_the_value_of_an_owned_field() {
        let (mut state, owned, _) = setup();
        let item = add_account(&mut state, owned, "copy-user");
        state
            .vault_manager
            .update_item_secret(
                owned,
                item,
                crate::SECRET_FIELD_PASSWORD,
                Some("s3cret".to_string()),
            )
            .unwrap();

        let value = state
            .item_secret_for_copy(owned, item, crate::SECRET_FIELD_PASSWORD)
            .unwrap();

        assert_eq!(value, "s3cret");
    }

    #[test]
    fn test_item_secret_for_copy_rejects_an_unowned_vault() {
        let (mut state, _, _) = setup();
        let foreign = add_unowned_unlocked_vault(&mut state);
        let item = add_account(&mut state, foreign, "foreign-user");
        state
            .vault_manager
            .update_item_secret(
                foreign,
                item,
                crate::SECRET_FIELD_PASSWORD,
                Some("s3cret".to_string()),
            )
            .unwrap();

        let result = state.item_secret_for_copy(foreign, item, crate::SECRET_FIELD_PASSWORD);

        assert!(
            matches!(result, Err(VaultError::Unauthorised)),
            "got {result:?}"
        );
    }

    #[test]
    fn test_item_secret_for_copy_reports_a_missing_value() {
        let (mut state, owned, _) = setup();
        let item = add_account(&mut state, owned, "no-secret");

        let result = state.item_secret_for_copy(owned, item, crate::SECRET_FIELD_PASSWORD);

        assert!(
            matches!(result, Err(VaultError::Validation(ref msg)) if msg.contains("no value")),
            "got {result:?}"
        );
    }

    #[test]
    fn test_get_all_items_excludes_vaults_the_active_user_does_not_own() {
        let (mut state, owned, _) = setup();
        add_account(&mut state, owned, "owned-user");

        let foreign = add_unowned_unlocked_vault(&mut state);
        add_account(&mut state, foreign, "foreign-user");

        let items = state
            .get_all_items(ItemFilter::default())
            .unwrap();

        assert_eq!(items.len(), 1, "only the owned vault's items are visible");
        assert_eq!(items[0].vault_id(), owned);
    }

    #[test]
    fn test_get_all_items_rejects_an_unowned_vault_filter() {
        let (mut state, _, _) = setup();
        let foreign = add_unowned_unlocked_vault(&mut state);

        let result = state.get_all_items(ItemFilter {
            vault_id: Some(foreign),
            ..ItemFilter::default()
        });

        assert!(matches!(result, Err(VaultError::Unauthorised)));
    }

    #[test]
    fn test_get_item_by_id_does_not_find_an_item_in_an_unowned_vault() {
        let (mut state, _, _) = setup();
        let foreign = add_unowned_unlocked_vault(&mut state);
        let foreign_item = add_account(&mut state, foreign, "foreign-user");

        let result = state.get_item_by_id(foreign_item);

        assert!(
            matches!(result, Err(VaultError::ItemNotFound(_))),
            "got {result:?}"
        );
    }

    #[test]
    fn test_get_unlocked_vaults_excludes_unowned_vaults() {
        let (mut state, owned, _) = setup();
        add_unowned_unlocked_vault(&mut state);

        let vaults = state.get_unlocked_vaults().unwrap();

        assert_eq!(vaults.len(), 1);
        assert_eq!(vaults[0].id, owned);
    }

    #[test]
    fn test_list_vault_ids_excludes_unowned_vaults() {
        let (mut state, owned, _) = setup();
        add_unowned_unlocked_vault(&mut state);

        let ids = state.list_vault_ids().unwrap();

        assert_eq!(ids, vec![owned.to_string()]);
    }

    // ============ Session timeouts ============

    #[test]
    fn test_session_expires_after_the_idle_timeout() {
        let (mut state, owned, _) = setup();
        state
            .last_activity
            .set(Some(Instant::now() - IDLE_TIMEOUT - Duration::from_secs(1)));

        assert!(state.expire_session());
        assert!(state.master_password.is_none());
        assert!(
            matches!(
                state.vault_manager.get_vault(owned),
                Err(VaultError::VaultLocked)
            ),
            "the timed-out session must leave no unlocked vault"
        );
        assert!(
            !state.expire_session(),
            "there is no session left to expire"
        );
    }

    #[test]
    fn test_recent_activity_keeps_the_session_alive() {
        let (state, _, _) = setup();
        state.last_activity.set(Some(Instant::now()));

        assert!(!state.session_timed_out());
    }

    #[test]
    fn test_absolute_lifetime_expires_even_while_active() {
        let (state, _, _) = setup();
        state
            .session_started
            .set(Some(Instant::now() - ABSOLUTE_TIMEOUT - Duration::from_secs(1)));
        state.last_activity.set(Some(Instant::now()));

        assert!(state.session_timed_out());
    }

    #[test]
    fn test_touch_extends_the_idle_clock() {
        let (state, _, _) = setup();
        let old = Instant::now() - Duration::from_secs(60);
        state.last_activity.set(Some(old));

        state.touch();

        assert!(state.last_activity.get().unwrap() > old);
    }

    // ============ Session switching ============

    #[test]
    fn test_end_session_locks_vaults_and_clears_credentials() {
        let (mut state, owned, _) = setup();

        state.end_session().unwrap();

        assert!(
            matches!(
                state.vault_manager.get_vault(owned),
                Err(VaultError::VaultLocked)
            ),
            "the previous session's vault must leave RAM"
        );
        assert!(state.master_password.is_none(), "the password must be dropped");
        assert!(state.secret_key.is_none(), "the secret key must be dropped");
        assert!(
            state.user_manager.active_user_id.is_none(),
            "no user may stay active"
        );
    }

    #[test]
    fn test_login_ends_the_previous_session() {
        let (mut state, owned, _) = setup();

        // A second profile becomes active without ending Alice's session:
        // the state a compromised renderer can create.
        let bob = state
            .user_manager
            .register("Bob".to_string(), "correct horse battery staple")
            .unwrap();
        assert!(
            state.vault_manager.get_vault(owned).is_ok(),
            "precondition: Alice's vault is still unlocked"
        );

        state
            .login_user(bob.id, "correct horse battery staple".to_string())
            .unwrap();

        assert!(
            matches!(
                state.vault_manager.get_vault(owned),
                Err(VaultError::VaultLocked)
            ),
            "logging in as Bob must lock Alice's vault"
        );
        assert!(state.get_unlocked_vaults().unwrap().is_empty());
    }

    #[test]
    fn test_failed_login_still_ends_the_previous_session() {
        let (mut state, owned, _) = setup();

        // The same precondition, but the new login is rejected: the teardown
        // must already have happened, not be skipped because authentication
        // failed.
        let bob = state
            .user_manager
            .register("Bob".to_string(), "correct horse battery staple")
            .unwrap();

        let result = state.login_user(bob.id, "WrongPassword".to_string());

        assert!(
            matches!(result, Err(VaultError::AuthenticationFailed)),
            "got {result:?}"
        );
        assert!(
            matches!(
                state.vault_manager.get_vault(owned),
                Err(VaultError::VaultLocked)
            ),
            "a failed login must still lock the previous vault"
        );
        assert!(state.master_password.is_none());
        assert!(state.secret_key.is_none());
        assert!(state.user_manager.active_user_id.is_none());
    }

    #[test]
    fn test_register_rejects_a_weak_password_without_ending_the_session() {
        let (mut state, owned, _) = setup();

        let result = state.register_user("Bob".to_string(), "short".to_string());

        assert!(
            matches!(result, Err(VaultError::Validation(_))),
            "got {result:?}"
        );
        assert!(
            state.vault_manager.get_vault(owned).is_ok(),
            "a rejected registration must leave the current session unlocked"
        );
        assert!(state.master_password.is_some());
    }

    #[test]
    fn test_register_ends_the_previous_session() {
        let (mut state, owned, _) = setup();

        let vaults = state
            .register_user(
                "Bob".to_string(),
                "correct horse battery staple".to_string(),
            )
            .unwrap();

        assert!(
            matches!(
                state.vault_manager.get_vault(owned),
                Err(VaultError::VaultLocked)
            ),
            "registering Bob must lock Alice's vault"
        );
        assert_eq!(vaults.len(), 1);
        assert_ne!(vaults[0].id, owned);
        assert_eq!(state.get_unlocked_vaults().unwrap().len(), 1);
    }
}
