use std::collections::HashMap;
use std::path::PathBuf;
use std::println;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use uuid::Uuid;
use zeroize::Zeroize;
use zxcvbn::{zxcvbn, Entropy};

use crate::mdns::DiscoverySession;
use crate::network::{NetworkError, PeerChannel, PeerNetwork, SyncMessage, TransportSession};
use crate::pairing::{PairingClaim, PAIRING_TTL_SECS};
use crate::sync::{
    PendingSync, SyncControl, SyncCredentials, SyncEngine, SyncOutcome, SyncProgress, SyncReport,
    SyncStateStore,
};
use crate::transport::TlsNetwork;
use crate::user_manager::UserManager;
use crate::vault_manager::VaultManager;
use crate::{
    DuplicateOutcome, DuplicateResolution, DuplicateSuggestion, IntoSafe, ItemFilter, NewItemData,
    PairingApprovalPrompt, PairingCode, PairingMessage, PairingOffer, SafeItem, SafeVault,
    SecretInput, User, UserId, VaultError, VaultId, CURRENT_SCHEMA_VERSION,
};

/// A one-way "stop waiting for pairing" flag.
///
/// Like [`SyncControl`], it lives outside the shared state so cancelling a
/// pairing command that is blocked on the network never waits for the lock
/// that command deliberately does not hold.
#[derive(Default)]
pub struct PairingControl {
    cancelled: AtomicBool,
}

impl PairingControl {
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

/// The source Device's live pairing state: the offer, its claim, and the one
/// authenticated connection a join arrived on.
///
/// The connection is deliberately the only thing held between the announcement
/// and the approval. There is no general session registry: when the offer
/// ends, the channel and the claim go with it.
struct ActivePairing {
    offer: PairingOffer,
    claim: PairingClaim,
    /// `None` until a join has been received and surfaced for approval.
    channel: Option<Box<dyn PeerChannel>>,
    /// Whether this offer opened advertising for itself. A discovery session
    /// already open owns its own lifecycle and must not be stopped here.
    owns_advertising: bool,
}

/// A running inbound-sync server.
struct ServingHandle {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ServingHandle {
    fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for ServingHandle {
    fn drop(&mut self) {
        // Never join here: a dropped handle may be dropped while the state
        // lock is held (identity recovery replaces the whole state), and the
        // server thread needs that lock to finish. Ask it to stop instead.
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub struct ManagerState {
    /// Where the managers read and write. Kept so an explicit identity
    /// recovery can re-initialise them without restarting the process.
    pub app_data_dir: PathBuf,

    /// `None` while a damaged Device identity is blocking startup. No command
    /// may touch vaults or profiles until the User chooses a recovery.
    pub vault_manager: Option<VaultManager>,
    pub user_manager: Option<UserManager>,

    /// Master password â€” only needed for create_vault / unlock_vault.
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
    pub network: Option<Arc<dyn PeerNetwork>>,

    /// The active discovery session. Dropping it stops advertising.
    pub discovery: Option<DiscoverySession>,

    /// Device-local Sync Set and last-sync timestamps. Never part of a Vault.
    pub sync_state: Option<SyncStateStore>,

    /// Conflicts holding a sync open, keyed by the report's session id.
    pub pending_syncs: HashMap<Uuid, PendingSync>,

    /// The live pairing offer, if one is open. Its channel is the only state
    /// carried between the join announcement and the human's approval.
    pairing: Option<ActivePairing>,

    /// The joining side's transfer, held in memory after approval until the
    /// local master password finishes the pairing. It never crosses to the UI.
    pending_join_transfer: Option<PairingMessage>,

    /// Whether the join opened advertising for itself.
    join_owns_advertising: bool,

    /// The running inbound-sync server, if any.
    serving: Option<ServingHandle>,
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
                // The production transport binds an ephemeral port now and
                // advertises it for the duration of a discovery or sync
                // session. If the socket cannot be bound, sync is reported as
                // unavailable rather than silently falling back to anything.
                let network: Option<Arc<dyn PeerNetwork>> =
                    match TlsNetwork::new(vault_manager.device_id()) {
                        Ok(transport) => Some(Arc::new(transport)),
                        Err(error) => {
                            eprintln!("Lockr could not start the peer transport: {error}");
                            None
                        }
                    };
                let sync_state = SyncStateStore::new(&app_data_dir).ok();
                Self {
                    app_data_dir,
                    vault_manager: Some(vault_manager),
                    user_manager: Some(user_manager),
                    master_password: None,
                    secret_key: None,
                    startup_error: None,
                    device_identity_damaged: false,
                    network,
                    discovery: None,
                    sync_state,
                    pending_syncs: HashMap::new(),
                    pairing: None,
                    pending_join_transfer: None,
                    join_owns_advertising: false,
                    serving: None,
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
                    pairing: None,
                    pending_join_transfer: None,
                    join_owns_advertising: false,
                    serving: None,
                }
            }
        }
    }

    /// Hand the transport the identity this Device proves on a paired channel
    /// and the Device Registry it must verify peers against.
    ///
    /// The registry is read here, at the start of a network session and after
    /// any local revocation, so the transport cannot authenticate a Device the
    /// User has since revoked. The sync engine independently re-checks the live
    /// registry at every frame boundary; this is the pre-connection refusal.
    fn configure_network_session(&self) -> Result<(), String> {
        let network = self.network.clone().ok_or_else(|| {
            VaultError::SyncTransportUnavailable("the peer transport is unavailable".into())
                .to_string()
        })?;
        let user_manager = self
            .user_manager
            .as_ref()
            .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
        let user_id = user_manager
            .active_user_id
            .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
        let keypair = user_manager
            .session_keypair()
            .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
        let devices = Arc::new(
            user_manager
                .get_shared_profile(&user_id)
                .map_err(|error| error.to_string())?
                .devices,
        );

        network
            .set_session(&TransportSession {
                device_id: user_manager.device_id(),
                keypair,
                devices,
            })
            .map_err(|error| error.to_string())
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

    /// Drop everything that belongs to one signed-in session's network and
    /// conflict state.
    ///
    /// A logout or a user switch must stop advertising (an idle app is
    /// invisible on the network) and forget the plaintext secret candidates an
    /// open conflict session was holding. Neither can ever be resolved once
    /// the session that produced them is gone.
    pub fn end_sync_session(&mut self) {
        if let Some(session) = self.discovery.take() {
            session.finish();
        }

        for (_, mut pending) in self.pending_syncs.drain() {
            pending.forget();
        }

        // The session signing key and registry snapshot must not outlive the
        // session that authorised them.
        if let Some(network) = &self.network {
            network.clear_session();
        }
    }

    /// Tear down any live pairing offer or join and leave nothing behind.
    ///
    /// This is the single teardown path for cancellation, expiry, a bad join,
    /// a failed approval and logout. It closes the authenticated channel,
    /// forgets the one-time claim, drops the in-memory transfer, and stops
    /// advertising only when pairing itself opened it.
    fn teardown_pairing(&mut self) {
        if let Some(mut active) = self.pairing.take() {
            if let Some(mut channel) = active.channel.take() {
                channel.close();
            }
            if let Some(network) = &self.network {
                let _ = network.end_pairing();
                if active.owns_advertising && self.discovery.is_none() {
                    let _ = network.stop_advertising();
                }
            }
        } else if let Some(network) = &self.network {
            // A join may have ended without an offer on this side.
            let _ = network.end_pairing();
        }

        if self.join_owns_advertising {
            self.join_owns_advertising = false;
            if self.discovery.is_none() {
                if let Some(network) = &self.network {
                    let _ = network.stop_advertising();
                }
            }
        }

        if let Some(transfer) = self.pending_join_transfer.take() {
            drop(transfer);
        }

        if let Some(user_manager) = self.user_manager.as_mut() {
            user_manager.cancel_pairing();
        }
    }

    /// Tear down a pairing offer only if it is still the one a waiting caller
    /// started. A stale waiter must not end a newer offer.
    fn teardown_pairing_for(&mut self, claim: &PairingClaim) {
        let still_current = self
            .pairing
            .as_ref()
            .is_some_and(|active| active.claim.matches_claim(claim));
        if still_current {
            self.teardown_pairing();
        }
    }

    /// Take the serving handle out of the state. The caller stops it after
    /// releasing the lock, because the server thread may need that lock to
    /// notice its stop flag.
    fn take_serving(&mut self) -> Option<ServingHandle> {
        self.serving.take()
    }

    fn network_handle(&self) -> Result<Arc<dyn PeerNetwork>, String> {
        self.network.clone().ok_or_else(|| {
            VaultError::SyncTransportUnavailable("the peer transport is unavailable".into())
                .to_string()
        })
    }

    /// Record what a completed exchange produced.
    ///
    /// The last-sync time is recorded even when conflicts hold the session
    /// open: the sync did happen, and a stale timestamp while the User works
    /// through the prompt would be a lie. The pending candidates are kept
    /// only for as long as the session that produced them.
    fn remember_sync_outcome(
        &mut self,
        user_id: UserId,
        peer_device_id: crate::DeviceId,
        outcome: SyncOutcome,
    ) -> Result<SyncReport, String> {
        let SyncOutcome { report, pending } = outcome;

        if let Some(store) = self.sync_state.as_mut() {
            store
                .record_sync(&user_id, peer_device_id, Utc::now())
                .map_err(|error| error.to_string())?;
        }

        if !pending.is_empty() {
            self.pending_syncs.insert(
                report.session_id,
                PendingSync {
                    user_id,
                    conflicts: pending,
                },
            );
        }

        Ok(report)
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

    state.end_sync_session();

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

    // Unlock all vaults â€” each derives its master key once (Argon2id).
    // After this, all further operations use the cached keys.
    for vault_id in &user.vault_ids {
        let _ = vault_manager.unlock_vault(*vault_id, mp, sk);
    }

    Ok(vault_manager.get_unlocked_vaults())
}

#[tauri::command]
pub fn logout(
    state: AppState,
    sync_control: State<'_, SyncControl>,
    pairing_control: State<'_, PairingControl>,
) -> Result<(), String> {
    // A blocked pairing wait may not hold the state lock; cancel it first.
    pairing_control.cancel();

    // Stop the server before taking the state lock: its thread needs that
    // same lock to notice the stop flag.
    stop_serving_blocking(state.inner(), sync_control.inner());

    let mut state = state.lock().map_err(|e| e.to_string())?;

    // Stop advertising and forget open conflict candidates before the
    // credentials that could resolve them are gone.
    state.end_sync_session();
    state.teardown_pairing();

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
pub fn delete_user(
    state: AppState,
    sync_control: State<'_, SyncControl>,
    pairing_control: State<'_, PairingControl>,
) -> Result<(), String> {
    pairing_control.cancel();
    stop_serving_blocking(state.inner(), sync_control.inner());

    let mut state = state.lock().map_err(|e| e.to_string())?;

    state.end_sync_session();
    state.teardown_pairing();

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

/// What the source Device reports after the live channel completed.
#[derive(Debug, Clone, Serialize)]
pub struct PairingOutcome {
    pub device_id: crate::DeviceId,
    pub name: String,
    pub platform: String,
}

/// What the joining Device reports once the source approved it. The transfer
/// itself stays in the backend; the UI only needs to know it may ask for the
/// local master password.
#[derive(Debug, Clone, Serialize)]
pub struct PairingJoinReady {
    pub source_device_id: crate::DeviceId,
    pub user_id: UserId,
    pub user_name: String,
}

fn pairing_cancelled() -> String {
    VaultError::Pairing("the pairing was cancelled".into()).to_string()
}

fn pairing_transport_error(error: NetworkError) -> String {
    match error {
        NetworkError::AuthenticationFailed => {
            VaultError::Pairing("the pairing code was rejected".into()).to_string()
        }
        NetworkError::Timeout => VaultError::PairingSecretStale.to_string(),
        other => other.to_string(),
    }
}

/// Mint the offer and start accepting joins for it.
fn run_start_pairing(
    state: &mut ManagerState,
    control: &PairingControl,
    now: DateTime<Utc>,
) -> Result<PairingOffer, String> {
    control.reset();

    // Any earlier offer is dead first: only one claim may ever be live.
    state.teardown_pairing();

    let offer = {
        let (_, user_manager, _, secret_key) = state.parts_mut()?;
        let secret_key = secret_key.as_ref().ok_or("Not logged in")?;
        user_manager
            .start_pairing(secret_key, CURRENT_SCHEMA_VERSION, now)
            .map_err(|e| e.to_string())?
    };

    let claim = offer.claim();
    let network = state.network_handle()?;
    network
        .begin_pairing(&claim)
        .map_err(sync_discovery_error)?;

    // Pairing needs to be discoverable while it waits, but it must not take
    // over a discovery session the Devices screen already owns.
    let owns_advertising = state.discovery.is_none();
    if let Err(error) = network.start_advertising() {
        // Drop the freshly minted host too: a failed offer must not leave its
        // secret key in memory waiting for a command that will never come.
        state.teardown_pairing();
        return Err(sync_discovery_error(error));
    }

    state.pairing = Some(ActivePairing {
        offer: offer.clone(),
        claim,
        channel: None,
        owns_advertising,
    });

    Ok(offer)
}

/// Wait for a join on the live transport and surface the announcement for
/// human approval. The authenticated channel is kept for [`run_approve_pairing`].
fn run_await_pairing_request(
    state: &Mutex<ManagerState>,
    control: &PairingControl,
) -> Result<PairingApprovalPrompt, String> {
    let (network, claim, expires_at) = {
        let state = state.lock().map_err(|e| e.to_string())?;
        let active = state
            .pairing
            .as_ref()
            .ok_or_else(|| VaultError::PairingNotStarted.to_string())?;
        (
            state.network_handle()?,
            active.claim.clone(),
            active.offer.expires_at,
        )
    };

    loop {
        if control.is_cancelled() {
            let mut state = state.lock().map_err(|e| e.to_string())?;
            state.teardown_pairing_for(&claim);
            return Err(pairing_cancelled());
        }

        let now = Utc::now();
        if now >= expires_at {
            let mut state = state.lock().map_err(|e| e.to_string())?;
            state.teardown_pairing_for(&claim);
            return Err(VaultError::PairingSecretStale.to_string());
        }

        match network.accept_for_pairing(&claim) {
            Ok(Some(mut channel)) => {
                let remaining = (expires_at - now)
                    .to_std()
                    .unwrap_or(Duration::ZERO)
                    .min(Duration::from_secs(5));
                match channel.receive_timeout(remaining) {
                    // A frame that is not a join on an authenticated pairing
                    // channel is a protocol violation, not a retryable state.
                    Ok(SyncMessage::Pairing(message)) if message.as_request().is_some() => {
                        let mut state = state.lock().map_err(|e| e.to_string())?;
                        if control.is_cancelled() || Utc::now() >= expires_at {
                            channel.close();
                            state.teardown_pairing_for(&claim);
                            return Err(if control.is_cancelled() {
                                pairing_cancelled()
                            } else {
                                VaultError::PairingSecretStale.to_string()
                            });
                        }

                        let prompt = match state.user_manager.as_mut() {
                            Some(user_manager) => {
                                user_manager.receive_pairing_request(message, Utc::now())
                            }
                            None => Err(VaultError::PairingNotStarted),
                        };

                        match prompt {
                            Ok(prompt) => {
                                let Some(active) = state.pairing.as_mut() else {
                                    channel.close();
                                    state.teardown_pairing_for(&claim);
                                    return Err(VaultError::PairingNotStarted.to_string());
                                };
                                active.channel = Some(channel);
                                return Ok(prompt);
                            }
                            Err(VaultError::PairingRateLimited) => {
                                // Two joins arrived too close together. The
                                // offer is still live; drop this channel and
                                // keep listening rather than burning it.
                                channel.close();
                            }
                            Err(error) => {
                                channel.close();
                                state.teardown_pairing_for(&claim);
                                return Err(error.to_string());
                            }
                        }
                    }
                    Ok(_) => {
                        channel.close();
                        let mut state = state.lock().map_err(|e| e.to_string())?;
                        state.teardown_pairing_for(&claim);
                        return Err(VaultError::Pairing(
                            "the pairing channel carried no join request".into(),
                        )
                        .to_string());
                    }
                    // A peer that authenticated and then went silent must not
                    // hold the offer: drop that channel and keep listening.
                    Err(NetworkError::Timeout) | Err(NetworkError::ConnectionClosed) => {
                        channel.close();
                    }
                    Err(error) => {
                        channel.close();
                        let mut state = state.lock().map_err(|e| e.to_string())?;
                        state.teardown_pairing_for(&claim);
                        return Err(pairing_transport_error(error));
                    }
                }
            }
            Ok(None) => continue,
            // The claim was burned by failed attempts or ended underneath us.
            Err(error) => {
                let mut state = state.lock().map_err(|e| e.to_string())?;
                state.teardown_pairing_for(&claim);
                return Err(pairing_transport_error(error));
            }
        }
    }
}

/// Approve the announced Device and deliver the transfer on the same live
/// connection. The frontend never sees or supplies the frame.
fn run_approve_pairing(
    state: &mut ManagerState,
    device_id: crate::DeviceId,
    sync_set: Option<Vec<String>>,
    now: DateTime<Utc>,
) -> Result<PairingOutcome, String> {
    let mut channel = state
        .pairing
        .as_mut()
        .and_then(|active| active.channel.take())
        .ok_or_else(|| VaultError::PairingNotStarted.to_string())?;

    let sync_set = sync_set
        .map(|ids| {
            ids.into_iter()
                .map(|id| Uuid::parse_str(&id).map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let (transfer, outcome) = {
        let (_, user_manager) = match state.ready_mut() {
            Ok(parts) => parts,
            Err(error) => {
                channel.close();
                state.teardown_pairing();
                return Err(error);
            }
        };

        let transfer = match user_manager.approve_pairing_with_sync_set(device_id, sync_set, now) {
            Ok(transfer) => transfer,
            Err(error) => {
                channel.close();
                state.teardown_pairing();
                return Err(error.to_string());
            }
        };

        let user_id = match user_manager.active_user_id {
            Some(user_id) => user_id,
            None => {
                channel.close();
                state.teardown_pairing();
                return Err(VaultError::SyncNotLoggedIn.to_string());
            }
        };
        let record = match user_manager
            .get_shared_profile(&user_id)
            .ok()
            .and_then(|shared| shared.devices.get(&device_id).cloned())
        {
            Some(record) => record,
            None => {
                channel.close();
                state.teardown_pairing();
                return Err(VaultError::Pairing(
                    "the approved Device is not in the registry".into(),
                )
                .to_string());
            }
        };

        (
            transfer,
            PairingOutcome {
                device_id,
                name: record.name,
                platform: record.platform,
            },
        )
    };

    if let Err(error) = channel.send(SyncMessage::Pairing(transfer)) {
        channel.close();
        state.teardown_pairing();
        return Err(pairing_transport_error(error));
    }
    channel.close();
    state.teardown_pairing();
    Ok(outcome)
}

/// Connect to the source Device, send the announcement, and wait for the
/// approval outcome on the same channel. Bounded by the offer's TTL.
fn run_join_pairing(
    state: &Mutex<ManagerState>,
    control: &PairingControl,
    code: &PairingCode,
    now: DateTime<Utc>,
) -> Result<PairingJoinReady, String> {
    control.reset();

    let (network, claim, source_device_id, join, deadline) = {
        let mut state = state.lock().map_err(|e| e.to_string())?;
        state.teardown_pairing();

        let claim = code.claim().map_err(|e| e.to_string())?;
        let source_device_id = code.source_device_id().map_err(|e| e.to_string())?;
        let deadline = code
            .expires_at()
            .map_err(|e| e.to_string())?
            .unwrap_or(now + chrono::TimeDelta::seconds(PAIRING_TTL_SECS));

        let join = {
            let (_, user_manager) = state.ready_mut()?;
            user_manager
                .join_pairing(code, CURRENT_SCHEMA_VERSION, now)
                .map_err(|e| e.to_string())?
        };

        let network = state.network_handle()?;
        if let Err(error) = network.start_advertising() {
            let _ = state.teardown_pairing();
            return Err(sync_discovery_error(error));
        }
        state.join_owns_advertising = state.discovery.is_none();

        (network, claim, source_device_id, join, deadline)
    };

    let abandon = |channel: Option<Box<dyn PeerChannel>>, error: String| -> String {
        if let Some(mut channel) = channel {
            channel.close();
        }
        if let Ok(mut state) = state.lock() {
            state.teardown_pairing();
        }
        error
    };

    // Discovery is live but mDNS resolving takes a moment. Retry until the
    // offer's own deadline, so a wrong Device id is reported as an expired
    // offer rather than hanging forever.
    let peer = loop {
        if control.is_cancelled() {
            return Err(abandon(None, pairing_cancelled()));
        }
        if Utc::now() >= deadline {
            return Err(abandon(None, VaultError::PairingSecretStale.to_string()));
        }
        match network.discover() {
            Ok(peers) => {
                if let Some(peer) = peers
                    .into_iter()
                    .find(|peer| peer.device_id() == source_device_id)
                {
                    break peer;
                }
            }
            Err(error) => return Err(abandon(None, sync_discovery_error(error))),
        }
        std::thread::sleep(Duration::from_millis(200));
    };

    let mut channel = match network.connect_for_pairing(&peer, &claim) {
        Ok(channel) => channel,
        Err(error) => return Err(abandon(None, pairing_transport_error(error))),
    };

    if let Err(error) = channel.send(SyncMessage::Pairing(join)) {
        return Err(abandon(Some(channel), pairing_transport_error(error)));
    }

    loop {
        if control.is_cancelled() {
            return Err(abandon(Some(channel), pairing_cancelled()));
        }
        if Utc::now() >= deadline {
            return Err(abandon(
                Some(channel),
                VaultError::PairingSecretStale.to_string(),
            ));
        }

        let remaining = (deadline - Utc::now())
            .to_std()
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_secs(1));
        match channel.receive_timeout(remaining) {
            Ok(SyncMessage::Pairing(message @ PairingMessage::Transfer(_))) => {
                let mut state = state.lock().map_err(|e| e.to_string())?;
                if control.is_cancelled() {
                    channel.close();
                    state.teardown_pairing();
                    return Err(pairing_cancelled());
                }

                let transfer = message
                    .as_transfer()
                    .expect("matched a transfer frame above");
                let ready = PairingJoinReady {
                    source_device_id,
                    user_id: transfer.shared_profile.user_id,
                    user_name: transfer.shared_profile.name.clone(),
                };
                state.pending_join_transfer = Some(message);

                // The joining side needs no discovery once the transfer is in
                // hand. The source's session, if any, is untouched.
                if state.join_owns_advertising {
                    state.join_owns_advertising = false;
                    if state.discovery.is_none() {
                        if let Some(network) = &state.network {
                            let _ = network.stop_advertising();
                        }
                    }
                }
                channel.close();
                return Ok(ready);
            }
            Ok(SyncMessage::Pairing(_)) => {
                return Err(abandon(
                    Some(channel),
                    VaultError::Pairing(
                        "the source Device sent a join request instead of a transfer".into(),
                    )
                    .to_string(),
                ));
            }
            Ok(other) => {
                return Err(abandon(
                    Some(channel),
                    VaultError::Pairing(format!(
                        "the pairing channel carried an unexpected frame: {other:?}"
                    ))
                    .to_string(),
                ));
            }
            Err(NetworkError::Timeout) => continue,
            Err(error) => return Err(abandon(Some(channel), pairing_transport_error(error))),
        }
    }
}

/// Finish the join with the password typed on this Device. The password never
/// leaves the machine.
fn run_complete_pairing(
    state: &mut ManagerState,
    master_password: String,
    now: DateTime<Utc>,
) -> Result<User, String> {
    let message = state
        .pending_join_transfer
        .take()
        .ok_or_else(|| VaultError::PairingNotStarted.to_string())?;

    // The source's pre-checked list becomes this Device's initial Sync Set.
    let initial_sync_set = message
        .as_transfer()
        .and_then(|transfer| transfer.sync_set.clone());

    let (_, user_manager, master_password_state, secret_key_state) = match state.parts_mut() {
        Ok(parts) => parts,
        Err(error) => {
            state.teardown_pairing();
            return Err(error);
        }
    };

    let user = match user_manager.complete_pairing(message, &master_password, now) {
        Ok(user) => user,
        Err(error) => {
            state.teardown_pairing();
            return Err(error.to_string());
        }
    };

    // A Device that has just paired is logged in on the strength of the
    // password typed here, exactly as if it had unlocked its own profile.
    let secret_key = match user_manager.login(&user.id, &master_password) {
        Ok(secret_key) => secret_key,
        Err(error) => {
            state.teardown_pairing();
            return Err(error.to_string());
        }
    };

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

    state.teardown_pairing();
    Ok(user)
}

/// Source Device: mint a single-use offer and start accepting a join over the
/// real transport. The secret key is held by the backend for the life of the
/// offer and is never part of the offer itself.
#[tauri::command]
pub fn start_pairing(
    state: AppState,
    control: State<'_, PairingControl>,
) -> Result<PairingOffer, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    run_start_pairing(&mut state, control.inner(), Utc::now())
}

/// Source Device: wait for the joining Device over the transport and return
/// the announcement the human must approve. Nothing is admitted and no key
/// material moves here.
#[tauri::command]
pub async fn receive_pairing_request(
    app: tauri::AppHandle,
) -> Result<PairingApprovalPrompt, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Mutex<ManagerState>>();
        let control = app.state::<PairingControl>();
        run_await_pairing_request(state.inner(), control.inner())
    })
    .await
    .map_err(|error| format!("the pairing wait did not finish: {error}"))?
}

/// Source Device: approve the Device shown in the prompt. The transfer is
/// written to the live channel; the frontend only receives the outcome.
#[tauri::command]
pub fn approve_pairing(
    state: AppState,
    device_id: String,
    sync_set: Option<Vec<String>>,
) -> Result<PairingOutcome, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let device_id = Uuid::parse_str(&device_id).map_err(|e| e.to_string())?;

    // The pre-checked list is the joining Device's initial Sync Set. `None`
    // leaves the default: every Vault in the shared registry.
    run_approve_pairing(&mut state, device_id, sync_set, Utc::now())
}

/// Abandon a live offer or an in-progress join, including a command currently
/// blocked waiting for the other Device.
#[tauri::command]
pub fn cancel_pairing(state: AppState, control: State<'_, PairingControl>) -> Result<(), String> {
    control.cancel();
    let mut state = state.lock().map_err(|e| e.to_string())?;
    state.teardown_pairing();
    Ok(())
}

/// Joining Device: connect with the scanned or typed code, announce this
/// Device, and wait for the source's approval. Nothing is written until the
/// password is typed locally.
#[tauri::command]
pub async fn join_pairing(
    app: tauri::AppHandle,
    code: PairingCode,
) -> Result<PairingJoinReady, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Mutex<ManagerState>>();
        let control = app.state::<PairingControl>();
        run_join_pairing(state.inner(), control.inner(), &code, Utc::now())
    })
    .await
    .map_err(|error| format!("the pairing join did not finish: {error}"))?
}

/// Joining Device: accept the transfer held by the backend and re-wrap the
/// secret key under the password typed locally.
#[tauri::command]
pub fn complete_pairing(state: AppState, master_password: String) -> Result<User, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    run_complete_pairing(&mut state, master_password, Utc::now())
}

// ============ Serving inbound sync ============

/// Serve at most one inbound sync, using the same engine path the initiator
/// uses. Returns whether a connection was served.
///
/// The engine's `accept` applies the same admission, registry, revocation and
/// scope rules as `start_with_peer`; nothing weaker is allowed in.
fn serve_one_inbound(
    state: &Mutex<ManagerState>,
    stop: &AtomicBool,
    control: &SyncControl,
    progress: &mut dyn FnMut(SyncProgress),
) -> Result<bool, String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

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

    state.configure_network_session()?;
    let network = state.network_handle()?;

    // Take a channel first, then claim the one-sync slot. Accepting is how the
    // transport hands us one connection at a time; the slot is what makes a
    // local sync and an inbound sync mutually exclusive.
    let accepted = {
        let ManagerState {
            vault_manager,
            user_manager,
            master_password,
            secret_key,
            ..
        } = &mut *state;

        let vault_manager = vault_manager.as_mut().ok_or_else(|| {
            VaultError::SyncRefused("vault manager is unavailable".into()).to_string()
        })?;
        let user_manager = user_manager.as_ref().ok_or_else(|| {
            VaultError::SyncRefused("user manager is unavailable".into()).to_string()
        })?;
        let master_password = master_password
            .as_deref()
            .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
        let secret_key = secret_key
            .as_deref()
            .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;

        let mut engine = SyncEngine::new(
            network.as_ref(),
            vault_manager,
            user_manager,
            sync_set.clone(),
            SyncCredentials {
                master_password,
                secret_key,
            },
        );
        engine.accept(progress).map_err(|error| error.to_string())?
    };

    let Some(accepted) = accepted else {
        return Ok(false);
    };

    // The session may have ended while the accept was waiting. Refuse the
    // channel rather than starting an exchange after logout.
    if stop.load(Ordering::SeqCst) {
        accepted.reject();
        return Ok(false);
    }

    // A local sync already owns the slot: refuse this inbound connection
    // rather than queueing it behind the other exchange.
    let lease = match control.begin() {
        Ok(lease) => lease,
        Err(error) => {
            accepted.reject();
            return Err(error.to_string());
        }
    };

    let result = (|| -> Result<SyncOutcome, VaultError> {
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
        )
        .with_cancel(lease.token());

        engine.receive_offers(accepted, progress)
    })();

    drop(lease);

    let outcome = result.map_err(|error| error.to_string())?;
    let peer_device_id = outcome.report.peer_device_id;
    state.remember_sync_outcome(user_id, peer_device_id, outcome)?;
    Ok(true)
}

/// Serve until stopped. A transient inbound failure is reported and does not
/// kill the server; losing the session does.
#[cfg(test)]
fn serve_loop(
    state: &Mutex<ManagerState>,
    stop: &AtomicBool,
    control: &SyncControl,
    mut progress: impl FnMut(SyncProgress),
) {
    while !stop.load(Ordering::SeqCst) {
        match serve_one_inbound(state, stop, control, &mut progress) {
            Ok(served) => {
                if !served {
                    // No listener yet (discovery has not opened one) or no
                    // connection arrived. Never spin on a missing socket.
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            Err(error) => {
                if error == VaultError::SyncNotLoggedIn.to_string() {
                    break;
                }
                eprintln!("Lockr could not serve an inbound sync: {error}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn serve_loop_app(app: tauri::AppHandle, stop: Arc<AtomicBool>) {
    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }

        let state = app.state::<Mutex<ManagerState>>();
        let control = app.state::<SyncControl>();
        let result = serve_one_inbound(state.inner(), &stop, control.inner(), &mut |progress| {
            let _ = app.emit("sync-progress", &progress);
        });

        match result {
            Ok(served) => {
                if !served {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            Err(error) => {
                if error == VaultError::SyncNotLoggedIn.to_string() {
                    break;
                }
                eprintln!("Lockr could not serve an inbound sync: {error}");
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    }
}

fn stop_serving_blocking(state: &Mutex<ManagerState>, control: &SyncControl) {
    let (handle, network) = match state.lock() {
        Ok(mut state) => (state.take_serving(), state.network.clone()),
        Err(poisoned) => {
            let mut state = poisoned.into_inner();
            (state.take_serving(), state.network.clone())
        }
    };

    control.cancel();
    if let Some(mut handle) = handle {
        handle.stop();
    }
    if let Some(network) = network {
        network.end_serving();
    }
}

/// Start serving inbound sync while a User is signed in.
///
/// This does not start advertising: visibility is owned by the discovery
/// session, exactly as it is for an outbound sync.
#[tauri::command]
pub fn start_serving(app: tauri::AppHandle, state: AppState) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;

    if state.serving.is_some() {
        return Ok(());
    }

    if state
        .user_manager
        .as_ref()
        .and_then(|manager| manager.active_user_id)
        .is_none()
    {
        return Err(VaultError::SyncNotLoggedIn.to_string());
    }

    if state.master_password.is_none() || state.secret_key.is_none() {
        return Err(VaultError::SyncNotLoggedIn.to_string());
    }

    state.configure_network_session()?;
    let network = state.network_handle()?;
    network.begin_serving().map_err(sync_discovery_error)?;

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread_app = app.clone();
    let handle = std::thread::Builder::new()
        .name("lockr-sync-server".to_string())
        .spawn(move || serve_loop_app(thread_app, thread_stop))
        .map_err(|error| format!("the sync server could not start: {error}"))?;

    state.serving = Some(ServingHandle {
        stop,
        handle: Some(handle),
    });
    Ok(())
}

/// Stop serving inbound sync. Any in-flight exchange is cancelled at its next
/// frame boundary.
#[tauri::command]
pub fn stop_serving(state: AppState, control: State<'_, SyncControl>) -> Result<(), String> {
    stop_serving_blocking(state.inner(), control.inner());
    Ok(())
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

/// Delete a Vault **everywhere**: record a Tombstone in the shared Vault
/// Registry and keep this Device's encrypted file as local trash.
///
/// This is the only one of the three removal operations that travels. It is
/// deliberately distinct from `set_vault_syncing(.., false)` (stop syncing:
/// the copy stays and drifts) and `remove_local_vault` (this Device only, no
/// Tombstone).
#[tauri::command]
pub fn delete_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let (vault_manager, user_manager) = state.ready_mut()?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    user_manager
        .delete_vault(id, Utc::now())
        .map_err(|e| e.to_string())?;

    // Never a hard delete on a deletion trigger: the User may have deleted
    // the wrong Vault, on any Device.
    vault_manager.move_to_trash(id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Remove this Device's copy of a Vault, and nothing else.
///
/// The Vault stays in the shared Vault Registry (other Devices keep theirs)
/// and no Tombstone is written. It also leaves this Device's Sync Set, because
/// leaving it there would let the next sync install it again from a peer.
#[tauri::command]
pub fn remove_local_vault(state: AppState, vault_id: String) -> Result<(), String> {
    let mut state = state.lock().map_err(|e| e.to_string())?;
    let id = Uuid::parse_str(&vault_id).map_err(|e| e.to_string())?;

    let user_id = state
        .user_manager
        .as_ref()
        .and_then(|manager| manager.active_user_id)
        .ok_or_else(|| VaultError::SyncNotLoggedIn.to_string())?;
    let registry = state
        .user_manager
        .as_ref()
        .expect("checked above")
        .get_shared_profile(&user_id)
        .map_err(|e| e.to_string())?
        .vaults;

    state
        .vault_manager
        .as_mut()
        .ok_or_else(|| "Lockr is not initialized".to_string())?
        .remove_local_copy(id)
        .map_err(|e| e.to_string())?;

    if let Some(store) = state.sync_state.as_mut() {
        store
            .set_vault_syncing(&user_id, id, false, &registry)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
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
/// When off, mutations only mark vaults dirty â€” you must call
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
///
/// The exchange blocks (it waits on the network and holds the vault state), so
/// the command is async and hands the work to Tauri's blocking pool. That
/// keeps the main thread free to dispatch `cancel_sync` while this runs.
#[tauri::command]
pub async fn sync_with_device(
    app: tauri::AppHandle,
    device_id: String,
) -> Result<SyncReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Mutex<ManagerState>>();
        let control = app.state::<SyncControl>();
        run_sync_with_device(&state, &control, &device_id, |progress| {
            let _ = app.emit("sync-progress", &progress);
        })
    })
    .await
    .map_err(|error| format!("the sync task did not finish: {error}"))?
}

/// The body of [`sync_with_device`], separate from Tauri so the command's
/// ordering can be tested: a second sync must be refused by the control
/// before it ever waits on the shared state.
fn run_sync_with_device(
    state: &Mutex<ManagerState>,
    control: &SyncControl,
    device_id: &str,
    mut progress: impl FnMut(SyncProgress),
) -> Result<SyncReport, String> {
    let peer_device_id: crate::DeviceId = Uuid::parse_str(device_id).map_err(|e| e.to_string())?;

    // Claim the single sync slot before touching the shared state. A second
    // invocation is refused here, immediately, instead of queueing behind the
    // first on that state and finding the slot free only once it finishes.
    let lease = control.begin().map_err(|error| error.to_string())?;

    let mut state = state.lock().map_err(|e| e.to_string())?;

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

    state.configure_network_session()?;

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
        )
        .with_cancel(lease.token());

        let start = engine.start_with_peer(&peer, &mut progress)?;
        let sent = engine.send_offers(start, &mut progress)?;
        engine.collect(sent, &mut progress)
    })();

    let _ = network.stop_advertising();
    let outcome = result.map_err(|error| error.to_string())?;

    state.remember_sync_outcome(user_id, peer_device_id, outcome)
}

/// Stop an in-flight sync at its next frame boundary.
#[tauri::command]
pub fn cancel_sync(control: tauri::State<'_, SyncControl>) -> Result<(), String> {
    control.cancel();
    Ok(())
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

    state.configure_network_session()?;

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

    let (scope, is_secret, expected_local, value) = {
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
            conflict.local_version,
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
        .resolve_conflict_field(
            vault_id,
            &scope,
            &field_key,
            is_secret,
            expected_local,
            value.as_ref(),
        )
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
    if shared.vaults.is_tombstoned(&vault_id) {
        // Deletion is permanent. Adopting the file again would resurrect a
        // Vault every Device has already deleted.
        return Err(VaultError::SyncRefused(format!(
            "Vault {vault_id} was deleted and cannot be adopted again"
        ))
        .to_string());
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
        .map_err(|e| e.to_string())?;

    // A transport session that was configured before the revocation must not
    // keep authenticating the revoked Device. Refresh its registry snapshot.
    // Best effort: the revocation itself has already succeeded and must not be
    // reported as failed because the transport is unavailable.
    let _ = state.configure_network_session();
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::mdns::{DiscoveryAdapter, DiscoveryEvent, ResolvedService, ServiceRecord};
    use crate::network::{InMemoryDeviceConfig, InMemoryHub};
    use crate::pairing::{PairingHost, PairingSecret, PAIRING_TTL_SECS};
    use crate::sync::PendingConflict;
    use crate::transport::TlsTransportConfig;

    fn setup() -> PathBuf {
        let temp_dir = std::env::temp_dir().join(format!("lockr_commands_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    // ============ Real-socket pairing and serving fixtures ============

    fn test_config() -> TlsTransportConfig {
        TlsTransportConfig {
            handshake_timeout: Duration::from_secs(3),
            frame_timeout: Duration::from_secs(3),
            accept_wait: Duration::from_millis(50),
        }
    }

    /// A discovery adapter that hands back exactly the peers a test seeds.
    /// It replaces mDNS so commands can be driven over real loopback sockets.
    #[derive(Default)]
    struct TestDiscovery {
        records: Mutex<Vec<ResolvedService>>,
    }

    impl TestDiscovery {
        fn publish(&self, device_id: crate::DeviceId, port: u16) {
            self.records.lock().unwrap().push(ResolvedService {
                service_type: crate::network::SYNC_SERVICE_TYPE.to_string(),
                fullname: format!("lockr-{device_id}._lockr-sync._tcp.local."),
                port,
                addresses: vec![std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)],
                properties: vec![("device_id".to_string(), device_id.to_string())],
            });
        }
    }

    impl DiscoveryAdapter for TestDiscovery {
        fn advertise(&self, _record: ServiceRecord) -> Result<(), NetworkError> {
            Ok(())
        }

        fn browse(&self) -> Result<(), NetworkError> {
            Ok(())
        }

        fn stop(&self) -> Result<(), NetworkError> {
            Ok(())
        }

        fn poll(&self) -> Vec<DiscoveryEvent> {
            self.records
                .lock()
                .unwrap()
                .drain(..)
                .map(DiscoveryEvent::Resolved)
                .collect()
        }
    }

    struct PairingLab {
        source: Mutex<ManagerState>,
        joiner: Mutex<ManagerState>,
        source_net: Arc<TlsNetwork>,
        joiner_net: Arc<TlsNetwork>,
        source_discovery: Arc<TestDiscovery>,
        joiner_discovery: Arc<TestDiscovery>,
        source_id: crate::DeviceId,
        joiner_id: crate::DeviceId,
        vault_id: VaultId,
        account_id: crate::ItemId,
        user_id: UserId,
        source_dirs: Vec<PathBuf>,
    }

    fn install_network(state: &mut ManagerState, discovery: Arc<TestDiscovery>) -> Arc<TlsNetwork> {
        let device_id = state
            .vault_manager
            .as_ref()
            .expect("the manager initialized")
            .device_id();
        let network = Arc::new(
            TlsNetwork::with_adapter(device_id, test_config(), discovery)
                .expect("the transport binds"),
        );
        state.network = Some(Arc::clone(&network) as Arc<dyn PeerNetwork>);
        network
    }

    const MASTER: &str = "Correct-Horse-Battery-Staple-9";
    const SECRET: &str = "the-secret-that-must-arrive";

    fn pairing_lab() -> PairingLab {
        let source_dir = setup();
        let joiner_dir = setup();

        let mut source = ManagerState::initialize(source_dir.clone());
        let mut joiner = ManagerState::initialize(joiner_dir.clone());

        let source_discovery = Arc::new(TestDiscovery::default());
        let joiner_discovery = Arc::new(TestDiscovery::default());
        let source_net = install_network(&mut source, Arc::clone(&source_discovery));
        let joiner_net = install_network(&mut joiner, Arc::clone(&joiner_discovery));

        let (user_id, source_key) = {
            let user_manager = source.user_manager.as_mut().unwrap();
            let user = user_manager.register("Alice".to_string(), MASTER).unwrap();
            let key = user_manager.login(&user.id, MASTER).unwrap();
            (user.id, key)
        };
        source.master_password = Some(MASTER.to_string());
        source.secret_key = Some(source_key.clone());

        let vault = source
            .vault_manager
            .as_mut()
            .unwrap()
            .create_vault(
                "Personal".to_string(),
                "#6240BF".to_string(),
                MASTER,
                &source_key,
            )
            .unwrap();
        source
            .user_manager
            .as_ref()
            .unwrap()
            .add_vault_to_user(vault.id)
            .unwrap();

        let account = source
            .vault_manager
            .as_mut()
            .unwrap()
            .add_item(
                vault.id,
                Some("Mail".to_string()),
                None,
                crate::NewItemData::Account(crate::AccountData {
                    username: "alice@example.test".to_string(),
                    email: None,
                }),
                Some(vec![crate::SecretInput {
                    key: "password".to_string(),
                    value: SECRET.to_string(),
                }]),
            )
            .unwrap();
        let account_id = match account {
            crate::SafeItem::Account(account) => account.id,
            other => panic!("expected an account, got {other:?}"),
        };

        PairingLab {
            source_id: source.vault_manager.as_ref().unwrap().device_id(),
            joiner_id: joiner.vault_manager.as_ref().unwrap().device_id(),
            source: Mutex::new(source),
            joiner: Mutex::new(joiner),
            source_net,
            joiner_net,
            source_discovery,
            joiner_discovery,
            vault_id: vault.id,
            account_id,
            user_id,
            source_dirs: vec![source_dir, joiner_dir],
        }
    }

    impl Drop for PairingLab {
        fn drop(&mut self) {
            for dir in self.source_dirs.drain(..) {
                std::fs::remove_dir_all(dir).ok();
            }
        }
    }

    /// Pair the two instances through the command bodies only: no caller ever
    /// supplies, alters or replays a handshake frame.
    fn pair_instances(lab: &PairingLab) -> PairingJoinReady {
        let source_control = PairingControl::default();
        let offer = {
            let mut source = lab.source.lock().unwrap();
            run_start_pairing(&mut source, &source_control, Utc::now()).unwrap()
        };

        lab.joiner_discovery
            .publish(lab.source_id, lab.source_net.local_port());

        let code = PairingCode::qr(offer.qr_payload.clone());
        let join_control = PairingControl::default();
        let (prompt_tx, prompt_rx) = mpsc::channel();

        let ready = std::thread::scope(|scope| {
            let await_handle = scope.spawn(|| {
                let result = run_await_pairing_request(&lab.source, &source_control);
                let _ = prompt_tx.send(result);
            });

            let join_handle =
                scope.spawn(|| run_join_pairing(&lab.joiner, &join_control, &code, Utc::now()));

            let prompt = prompt_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("the source surfaces the join")
                .expect("the join is authentic");

            let outcome = {
                let mut source = lab.source.lock().unwrap();
                run_approve_pairing(&mut source, prompt.device_id, None, Utc::now()).unwrap()
            };
            assert_eq!(outcome.device_id, lab.joiner_id);

            let ready = join_handle
                .join()
                .expect("the join thread finished")
                .expect("the joining side receives the approval outcome");
            await_handle.join().expect("the wait thread finished");
            ready
        });

        {
            let mut joiner = lab.joiner.lock().unwrap();
            let user = run_complete_pairing(&mut joiner, MASTER.to_string(), Utc::now()).unwrap();
            assert_eq!(user.id, lab.user_id);
        }

        ready
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

    fn conflict_side(device: crate::DeviceId) -> crate::ConflictSide {
        crate::ConflictSide {
            device,
            counter: 1,
            value: None,
        }
    }

    fn pending_conflict() -> PendingConflict {
        PendingConflict {
            vault_id: Uuid::new_v4(),
            scope: crate::ConflictScope::Vault,
            field_key: "name".to_string(),
            is_secret: false,
            local: Some(crate::merge::FieldValue::Plain("Mine".to_string())),
            remote: Some(crate::merge::FieldValue::Plain("Theirs".to_string())),
            local_version: None,
            sides: [conflict_side(Uuid::new_v4()), conflict_side(Uuid::new_v4())],
        }
    }

    #[test]
    fn test_a_second_sync_is_refused_from_the_command_path_without_queueing() {
        let temp_dir = setup();
        let state = Mutex::new(ManagerState::initialize(temp_dir.clone()));
        let control = SyncControl::default();

        // A first sync is in flight: it owns the one sync slot and the shared
        // state, exactly as the command does for the duration of an exchange.
        let first = control.begin().unwrap();
        let held = state.lock().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();

        std::thread::scope(|scope| {
            scope.spawn(|| {
                let result =
                    run_sync_with_device(&state, &control, &Uuid::new_v4().to_string(), |_| {});
                let _ = sender.send(result.map(|_| ()));
            });

            let received = receiver.recv_timeout(std::time::Duration::from_secs(2));
            // Release the probe first: if the command wrongly queued on the
            // state, this lets it finish instead of hanging the failing test.
            drop(held);
            let result = received.expect("a second sync must be refused, not queued");
            assert!(
                matches!(result, Err(ref message) if message.contains("already in progress")),
                "unexpected result: {result:?}"
            );
        });

        drop(first);
        assert!(
            !control.is_active(),
            "a refused sync must not claim the slot"
        );
        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_ending_a_session_stops_advertising_and_forgets_pending_conflicts() {
        let temp_dir = setup();
        let mut state = ManagerState::initialize(temp_dir.clone());

        // Swap the real mDNS seam for an in-memory one so advertising is
        // observable from a peer in this process.
        let hub = InMemoryHub::new();
        let device_id = state.vault_manager.as_ref().unwrap().device_id();
        let network: Arc<dyn PeerNetwork> =
            Arc::new(hub.join(InMemoryDeviceConfig::new(device_id, 3)));
        let observer = hub.join(InMemoryDeviceConfig::new(Uuid::new_v4(), 3));
        state.network = Some(Arc::clone(&network));
        state.discovery = Some(DiscoverySession::start(&network).unwrap());
        assert_eq!(
            observer.discover().unwrap().len(),
            1,
            "an open discovery session advertises"
        );

        state.pending_syncs.insert(
            Uuid::new_v4(),
            PendingSync {
                user_id: Uuid::new_v4(),
                conflicts: vec![pending_conflict()],
            },
        );

        state.end_sync_session();

        assert!(
            observer.discover().unwrap().is_empty(),
            "ending the session must stop advertising"
        );
        assert!(state.discovery.is_none());
        assert!(
            state.pending_syncs.is_empty(),
            "plaintext conflict candidates must not outlive the session"
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_last_sync_is_recorded_even_when_conflicts_hold_the_session_open() {
        let temp_dir = setup();
        let mut state = ManagerState::initialize(temp_dir.clone());
        let user_id = Uuid::new_v4();
        let peer_device_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        let outcome = SyncOutcome {
            report: SyncReport {
                session_id,
                peer_device_id,
                vault_ids: vec![Uuid::new_v4()],
                conflicts: Vec::new(),
                duplicate_notes: Vec::new(),
            },
            pending: vec![pending_conflict()],
        };

        let report = state
            .remember_sync_outcome(user_id, peer_device_id, outcome)
            .unwrap();

        assert_eq!(report.session_id, session_id);
        assert!(
            state
                .sync_state
                .as_ref()
                .unwrap()
                .last_sync_at(&user_id, peer_device_id)
                .is_some(),
            "a sync that surfaced a Conflict still happened"
        );
        assert_eq!(
            state
                .pending_syncs
                .get(&session_id)
                .map(|pending| pending.conflicts.len()),
            Some(1)
        );

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_two_instances_pair_over_the_commands_without_caller_supplied_frames() {
        let lab = pairing_lab();

        let ready = pair_instances(&lab);

        assert_eq!(ready.user_id, lab.user_id);
        assert_eq!(ready.source_device_id, lab.source_id);

        let source_key = lab.source.lock().unwrap().secret_key.clone().unwrap();
        let joiner_key = lab.joiner.lock().unwrap().secret_key.clone().unwrap();
        assert_eq!(
            joiner_key, source_key,
            "the secret key arrives over the real pairing channel"
        );

        // Nothing is left behind on either side: no offer, no held channel,
        // no transfer waiting for a password.
        let source = lab.source.lock().unwrap();
        assert!(source.pairing.is_none());
        assert!(source
            .user_manager
            .as_ref()
            .unwrap()
            .active_user_id
            .is_some());
        drop(source);

        let joiner = lab.joiner.lock().unwrap();
        assert!(joiner.pairing.is_none());
        assert!(joiner.pending_join_transfer.is_none());
        assert!(!joiner.join_owns_advertising);
    }

    #[test]
    fn test_sync_from_one_side_completes_against_a_serving_side() {
        let lab = pairing_lab();
        pair_instances(&lab);

        // Serving starts only because this test opens advertising; the server
        // command itself never advertises.
        lab.source_discovery
            .publish(lab.joiner_id, lab.joiner_net.local_port());
        lab.joiner_net.start_advertising().unwrap();
        lab.joiner_net.begin_serving().unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let server_control = SyncControl::default();
        let source_control = SyncControl::default();

        std::thread::scope(|scope| {
            let server = scope.spawn(|| serve_loop(&lab.joiner, &stop, &server_control, |_| {}));

            let report = run_sync_with_device(
                &lab.source,
                &source_control,
                &lab.joiner_id.to_string(),
                |_| {},
            )
            .expect("the initiator completes against the serving side");
            assert_eq!(report.peer_device_id, lab.joiner_id);

            stop.store(true, Ordering::SeqCst);
            server_control.cancel();
            server.join().expect("the server loop stops");
        });

        let joiner = lab.joiner.lock().unwrap();
        let vault_manager = joiner.vault_manager.as_ref().unwrap();
        assert_eq!(
            vault_manager
                .get_item_secret(lab.vault_id, lab.account_id, "password")
                .unwrap()
                .as_deref(),
            Some(SECRET),
            "the secret arrives intact over the server path"
        );
        assert!(
            vault_manager
                .get_vault(lab.vault_id)
                .unwrap()
                .items
                .iter()
                .any(|item| matches!(item.kind, Some(crate::ItemKind::Account(_)))),
            "the Vault itself arrived, not only the registry entry"
        );
    }

    #[test]
    fn test_an_expired_code_tears_down_the_join_without_leaving_state() {
        let lab = pairing_lab();
        let past = Utc::now() - chrono::TimeDelta::seconds(PAIRING_TTL_SECS + 1);
        let (_, offer) = PairingHost::start(
            Uuid::new_v4(),
            lab.source_id,
            CURRENT_SCHEMA_VERSION,
            [7u8; 32],
            past,
        );
        let code = PairingCode::qr(offer.qr_payload);
        let control = PairingControl::default();

        let error = run_join_pairing(&lab.joiner, &control, &code, Utc::now())
            .expect_err("an expired code must not join");
        assert!(error.contains("stale"), "unexpected error: {error}");

        let joiner = lab.joiner.lock().unwrap();
        assert!(joiner.pending_join_transfer.is_none());
        assert!(!joiner.join_owns_advertising);
        assert!(
            joiner
                .user_manager
                .as_ref()
                .unwrap()
                .active_user_id
                .is_none(),
            "an expired code must not write anything"
        );
    }

    #[test]
    fn test_a_wrong_code_is_refused_and_leaves_no_join_state() {
        let lab = pairing_lab();
        let source_control = PairingControl::default();
        let offer = {
            let mut source = lab.source.lock().unwrap();
            run_start_pairing(&mut source, &source_control, Utc::now()).unwrap()
        };
        lab.joiner_discovery
            .publish(lab.source_id, lab.source_net.local_port());

        let wrong_code = loop {
            let candidate = PairingSecret::generate().typed_code();
            if candidate != offer.typed_code {
                break candidate;
            }
        };

        let join_control = PairingControl::default();
        let code = PairingCode::typed(lab.source_id, wrong_code);
        let error = run_join_pairing(&lab.joiner, &join_control, &code, Utc::now())
            .expect_err("a wrong code must not join");
        assert!(error.contains("rejected"), "unexpected error: {error}");

        let joiner = lab.joiner.lock().unwrap();
        assert!(joiner.pending_join_transfer.is_none());
        assert!(!joiner.join_owns_advertising);
        assert!(joiner
            .user_manager
            .as_ref()
            .unwrap()
            .active_user_id
            .is_none());
        drop(joiner);

        // The accepted trade-off: one wrong connection counts against the
        // offer's attempt budget but does not burn it, so the human can retype.
        let mut source = lab.source.lock().unwrap();
        assert!(source.pairing.is_some());
        source.teardown_pairing();
    }

    #[test]
    fn test_cancelling_a_waiting_join_tears_down_both_sides_and_the_offer() {
        let lab = pairing_lab();

        let source_control = PairingControl::default();
        let offer = {
            let mut source = lab.source.lock().unwrap();
            run_start_pairing(&mut source, &source_control, Utc::now()).unwrap()
        };
        let claim = PairingCode::qr(offer.qr_payload.clone()).claim().unwrap();
        lab.joiner_discovery
            .publish(lab.source_id, lab.source_net.local_port());

        let code = PairingCode::qr(offer.qr_payload.clone());
        let join_control = PairingControl::default();
        let (prompt_tx, prompt_rx) = mpsc::channel();

        std::thread::scope(|scope| {
            let await_handle = scope.spawn(|| {
                let result = run_await_pairing_request(&lab.source, &source_control);
                let _ = prompt_tx.send(result);
            });
            let join_handle =
                scope.spawn(|| run_join_pairing(&lab.joiner, &join_control, &code, Utc::now()));

            let prompt = prompt_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("the announcement arrives")
                .expect("the join is authentic");
            assert_eq!(prompt.device_id, lab.joiner_id);

            // The human never approves: cancel and tear the channel down.
            source_control.cancel();
            {
                let mut source = lab.source.lock().unwrap();
                source.teardown_pairing();
                assert!(source.pairing.is_none());
            }

            let join_error = join_handle
                .join()
                .expect("the join thread finished")
                .expect_err("a cancelled pairing must not complete");
            assert!(
                !join_error.is_empty(),
                "the joining side reports why it stopped"
            );
            await_handle.join().expect("the wait thread finished");

            assert!(
                lab.source_net.accept_for_pairing(&claim).is_err(),
                "the cancelled offer must not accept another join"
            );
        });

        let joiner = lab.joiner.lock().unwrap();
        assert!(joiner.pending_join_transfer.is_none());
        assert!(!joiner.join_owns_advertising);
    }
}
