//! Replication: the Sync Set, whole encrypted vault exchange, and conflicts.
//!
//! The engine is deliberately small and transport-free. It speaks the
//! [`PeerChannel`] seam opened by a [`PeerNetwork`], exchanges each side's
//! Sync Set and shared registries before any vault blob, then sends whole
//! encrypted vault files for the intersection. Each side merges locally with
//! the pure merge engine from `merge.rs`; no plaintext ever crosses the
//! channel.
//!
//! The protocol is a four-step state machine, so two in-memory Devices can
//! drive it deterministically with no threads:
//!
//! ```text
//! initiator.start_with_peer      -> Scope(local)
//! responder.accept               <- Scope(initiator)
//!                                -> Scope(responder)
//! initiator.send_offers          <- Scope(responder)
//!                                -> Scope(revised)   (only if the merge grew
//!                                                     the initiator's scope)
//!                                -> Vault* then Complete
//! responder.receive_offers       <- Scope(revised) then Vault* then Complete
//!                                -> Vault* then Complete
//! initiator.collect              <- Vault* then Complete
//! ```
//!
//! The initiator sends first at every phase, and the responder replies, so a
//! queue-backed test channel never has to guess when to block.
//!
//! The revised scope is what lets a Vault created on the responder since the
//! last sync transfer in one round: the initiator only learns the Vault from
//! the responder's registry, so it must announce that it now wants it before
//! the responder decides what to offer.
//!
//! ## Conflicts are saved differently from clean merges
//!
//! A merge returns a deterministic preview for contested fields so both
//! Devices compute the same report. That preview is *never* written to disk.
//! [`crate::merge::keep_local_for_conflicts`] restores each contested field -
//! value and clock - from the local vault while keeping every clean merge. The
//! sync result surfaces the conflict, and only an explicit resolution writes
//! the chosen value.
//!
//! ## Scope and the Vault Registry
//!
//! The Vault Registry is authoritative on which Vaults exist. Discovery and
//! exchange only ever consider ids that are in the (merged) registry *and* in
//! this Device's Sync Set *and* in the peer's Sync Set. A `.vault` file that is
//! absent from the registry is invisible to sync until explicitly adopted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroize;

use crate::atomic::write_atomic;
use crate::merge::FieldValue;
use crate::network::{
    DiscoveredPeer, EncryptedVaultBlob, NetworkError, PeerChannel, PeerNetwork, ScopeState,
    SyncMessage,
};
use crate::registry::{SharedRegistries, VaultRegistry};
use crate::user_manager::UserManager;
use crate::vault_manager::VaultManager;
use crate::{
    Conflict, ConflictScope, DeviceId, FieldVersion, ItemId, MergeError, MergeOutcome, UserId, Vault,
    VaultError, VaultId, CURRENT_SCHEMA_VERSION,
};

/// Device-local file holding Sync Sets and last-sync timestamps. It is never
/// part of a Vault and never travels; each Device decides its own scope.
const SYNC_STATE_FILE: &str = "sync_state.json";

/// How long the engine waits for one frame from the peer.
///
/// A sync is a short conversational exchange, so a healthy peer replies
/// promptly. A silent one must fail rather than leave the command — and every
/// other command waiting on the same state — blocked indefinitely.
pub const SYNC_FRAME_TIMEOUT: Duration = Duration::from_secs(30);


/// The Sync Set rule for one Device.
///
/// `All` is the default for a freshly paired Device: the Vault Registry's ids,
/// evaluated on every use so a Vault added after pairing is included. Once the
/// User edits the set it becomes `Explicit` and stays exactly what they chose.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSet {
    explicit: Option<BTreeSet<VaultId>>,
}

impl SyncSet {
    /// The pre-checked default: every Vault in the shared registry.
    pub fn all() -> Self {
        Self { explicit: None }
    }

    /// The User's explicit choice.
    pub fn explicit(vault_ids: impl IntoIterator<Item = VaultId>) -> Self {
        Self {
            explicit: Some(vault_ids.into_iter().collect()),
        }
    }

    pub fn is_explicit(&self) -> bool {
        self.explicit.is_some()
    }

    /// Resolve the rule against the authoritative registry.
    pub fn ids(&self, registry: &VaultRegistry) -> BTreeSet<VaultId> {
        match &self.explicit {
            None => registry.iter().copied().collect(),
            Some(ids) => ids
                .iter()
                .copied()
                .filter(|id| registry.contains(id))
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct PeerSyncState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_sync_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct UserSyncState {
    /// `None` is the "all registry Vaults" rule; `Some` is the User's edit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sync_set: Option<BTreeSet<VaultId>>,

    #[serde(default)]
    peers: BTreeMap<DeviceId, PeerSyncState>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct DeviceSyncState {
    #[serde(default)]
    users: BTreeMap<UserId, UserSyncState>,
}

/// Persisted, device-local sync configuration.
pub struct SyncStateStore {
    path: PathBuf,
    state: DeviceSyncState,
}

impl SyncStateStore {
    /// Load the store from `app_data_dir`, or start empty if this Device has
    /// never synced.
    pub fn new(app_data_dir: &Path) -> Result<Self, VaultError> {
        Self::load(app_data_dir.join(SYNC_STATE_FILE))
    }

    pub fn load(path: PathBuf) -> Result<Self, VaultError> {
        if !path.exists() {
            return Ok(Self {
                path,
                state: DeviceSyncState::default(),
            });
        }

        let json = std::fs::read_to_string(&path)?;
        let state = serde_json::from_str(&json)?;
        Ok(Self { path, state })
    }

    /// The rule for this User. Absent state means the freshly paired default.
    pub fn scope(&self, user_id: &UserId) -> SyncSet {
        match self
            .state
            .users
            .get(user_id)
            .and_then(|user| user.sync_set.clone())
        {
            None => SyncSet::all(),
            Some(ids) => SyncSet::explicit(ids),
        }
    }

    /// The resolved Sync Set for display and for the sync engine.
    pub fn sync_set(&self, user_id: &UserId, registry: &VaultRegistry) -> BTreeSet<VaultId> {
        self.scope(user_id).ids(registry)
    }

    /// Replace the Sync Set with an explicit choice.
    pub fn set_sync_set(
        &mut self,
        user_id: &UserId,
        vault_ids: impl IntoIterator<Item = VaultId>,
    ) -> Result<(), VaultError> {
        let user = self.user_mut(user_id);
        user.sync_set = Some(vault_ids.into_iter().collect());
        self.persist()
    }

    /// Add or remove one Vault from the set, materialising the default first.
    pub fn set_vault_syncing(
        &mut self,
        user_id: &UserId,
        vault_id: VaultId,
        syncing: bool,
        registry: &VaultRegistry,
    ) -> Result<(), VaultError> {
        let mut ids = self.sync_set(user_id, registry);
        if syncing {
            ids.insert(vault_id);
        } else {
            ids.remove(&vault_id);
        }
        self.set_sync_set(user_id, ids)
    }

    /// Adoption shares the Vault, so it joins the Sync Set as well as the
    /// registry. Never called automatically.
    pub fn adopt(
        &mut self,
        user_id: &UserId,
        vault_id: VaultId,
        registry: &VaultRegistry,
    ) -> Result<(), VaultError> {
        let mut ids = self.sync_set(user_id, registry);
        ids.insert(vault_id);
        self.set_sync_set(user_id, ids)
    }

    /// Record a completed sync with one peer.
    pub fn record_sync(
        &mut self,
        user_id: &UserId,
        peer_device_id: DeviceId,
        at: DateTime<Utc>,
    ) -> Result<(), VaultError> {
        self.user_mut(user_id)
            .peers
            .entry(peer_device_id)
            .or_default()
            .last_sync_at = Some(at);
        self.persist()
    }

    pub fn last_sync_at(
        &self,
        user_id: &UserId,
        peer_device_id: DeviceId,
    ) -> Option<DateTime<Utc>> {
        self.state
            .users
            .get(user_id)
            .and_then(|user| user.peers.get(&peer_device_id))
            .and_then(|peer| peer.last_sync_at)
    }

    fn user_mut(&mut self, user_id: &UserId) -> &mut UserSyncState {
        self.state.users.entry(*user_id).or_default()
    }

    fn persist(&self) -> Result<(), VaultError> {
        let json = serde_json::to_string_pretty(&self.state)?;
        write_atomic(&self.path, json.as_bytes())?;
        Ok(())
    }
}

/// Credentials already resident in the session. The engine never reads a
/// password from anywhere else, and refuses to run while logged out.
#[derive(Clone, Copy)]
pub struct SyncCredentials<'a> {
    pub master_password: &'a str,
    pub secret_key: &'a [u8],
}

/// A one-way "stop this sync" flag.
///
/// It lives outside the global application state on purpose: the UI must be
/// able to cancel a sync even while the command that is running it holds that
/// state. The engine checks it between frames; a blocking transport should
/// also use it to cut a read short.
#[derive(Debug, Clone, Default)]
pub struct SyncCancel {
    flag: Arc<AtomicBool>,
}

impl SyncCancel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

/// Guards the "one sync at a time" rule and hands out its cancel token.
///
/// This is separate from [`crate::commands::ManagerState`] so that starting a
/// sync, cancelling one, and noticing that another is already running never
/// need the same lock the running exchange holds.
#[derive(Default)]
pub struct SyncControl {
    active: Mutex<Option<SyncCancel>>,
}

impl SyncControl {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<SyncCancel>> {
        // The value is a single flag; recovering a poisoned lock is safe and
        // means a panic elsewhere cannot wedge every later sync.
        self.active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Claim the single sync slot. Two simultaneous syncs must not interleave:
    /// the second caller is refused rather than racing the first over vault
    /// files. Dropping the lease releases the slot.
    pub fn begin(&self) -> Result<SyncLease<'_>, VaultError> {
        let mut active = self.lock();
        if active.is_some() {
            return Err(VaultError::SyncRefused(
                "a sync is already in progress on this Device".into(),
            ));
        }

        let token = SyncCancel::new();
        *active = Some(token.clone());

        Ok(SyncLease {
            control: self,
            token,
        })
    }

    /// Ask the running sync, if any, to stop at the next frame boundary.
    pub fn cancel(&self) {
        if let Some(token) = self.lock().as_ref() {
            token.cancel();
        }
    }

    pub fn is_active(&self) -> bool {
        self.lock().is_some()
    }

    fn finish(&self) {
        *self.lock() = None;
    }
}

/// A claimed sync slot. Dropping it lets the next sync begin, even if the
/// exchange failed or panicked.
pub struct SyncLease<'a> {
    control: &'a SyncControl,
    token: SyncCancel,
}

impl SyncLease<'_> {
    /// The token to hand to [`SyncEngine::with_cancel`].
    pub fn token(&self) -> SyncCancel {
        self.token.clone()
    }
}

impl Drop for SyncLease<'_> {
    fn drop(&mut self) {
        self.control.finish();
    }
}

/// Progress reported by the engine. The UI receives the same shape as Tauri
/// events; the engine itself only knows this callback, so it is callable
/// without a UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum SyncProgress {
    Started {
        peer_device_id: DeviceId,
        vault_ids: Vec<VaultId>,
    },
    ScopeAgreed {
        peer_device_id: DeviceId,
        vault_ids: Vec<VaultId>,
    },
    SendingVault {
        vault_id: VaultId,
        bytes: usize,
    },
    ReceivingVault {
        vault_id: VaultId,
        bytes: usize,
    },
    VaultMerged {
        vault_id: VaultId,
    },
    VaultConflicted {
        vault_id: VaultId,
        conflicts: usize,
    },
    Complete {
        vaults: usize,
        conflicts: usize,
    },
}

/// One surfaced Conflict, carrying the Vault id externally.
///
/// `ConflictScope::Vault` has no id by design (metadata belongs to the whole
/// Vault), but the UI needs to know which Vault the prompt is about.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncConflict {
    pub vault_id: VaultId,

    #[serde(flatten)]
    pub conflict: Conflict,
}

/// A same item identity found live in more than one local Vault after a sync.
///
/// This is the survivable #47 limitation: two Devices moving one Item to two
/// different destinations at the same time can leave a live copy in each.
/// Sync does not attempt to pick a winner; it surfaces the identity for
/// Duplicate review so nothing is silently discarded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DuplicateIdentityNote {
    pub item_id: ItemId,
    pub vault_ids: Vec<VaultId>,
}

/// What one completed sync produced.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncReport {
    /// Identifies this sync while unresolved Conflicts hold it open.
    pub session_id: Uuid,
    pub peer_device_id: DeviceId,
    pub vault_ids: Vec<VaultId>,

    /// Safe-view Conflicts. Secret values are never present.
    pub conflicts: Vec<SyncConflict>,

    /// Same identity live in multiple Vaults; review as a Duplicate.
    pub duplicate_notes: Vec<DuplicateIdentityNote>,
}

/// Internal companion to [`SyncReport`]: the candidate values a resolution
/// must choose between. It never crosses the network and is never serialized
/// to the UI, because secret candidates belong to the backend only.
#[derive(Debug, Clone)]
pub(crate) struct PendingConflict {
    pub(crate) vault_id: VaultId,
    pub(crate) scope: ConflictScope,
    pub(crate) field_key: String,
    pub(crate) is_secret: bool,
    pub(crate) local: Option<FieldValue>,
    pub(crate) remote: Option<FieldValue>,

    /// This Device's field version at the moment the Conflict was surfaced.
    /// A resolution is refused if the field has been written since, because
    /// the candidate the User was shown would clobber newer work.
    pub(crate) local_version: Option<FieldVersion>,

    /// The safe provenance the merge engine produced, reused verbatim in the
    /// surfaced report so the UI can say which Device wrote which side.
    pub(crate) sides: [crate::ConflictSide; 2],
}

#[derive(Debug, Clone)]
pub(crate) struct PendingSync {
    pub(crate) user_id: UserId,
    pub(crate) conflicts: Vec<PendingConflict>,
}

impl PendingSync {
    /// Forget the plaintext secret candidates this open session held.
    ///
    /// A logout or a user switch must not leave secret values sitting in
    /// memory waiting for a resolution that can never arrive.
    pub(crate) fn forget(&mut self) {
        for conflict in &mut self.conflicts {
            for candidate in [&mut conflict.local, &mut conflict.remote] {
                if let Some(FieldValue::Secret(secret)) = candidate {
                    secret.zeroize();
                }
                *candidate = None;
            }
        }
        self.conflicts.clear();
    }
}

/// The engine's full result: the safe report and the backend-only candidates.
#[derive(Debug, Clone)]
pub struct SyncOutcome {
    pub report: SyncReport,
    pub(crate) pending: Vec<PendingConflict>,
}

impl SyncOutcome {
    fn report_only(session_id: Uuid, peer_device_id: DeviceId) -> Self {
        Self {
            report: SyncReport {
                session_id,
                peer_device_id,
                vault_ids: Vec::new(),
                conflicts: Vec::new(),
                duplicate_notes: Vec::new(),
            },
            pending: Vec::new(),
        }
    }
}

/// A connected initiator waiting for the responder's Scope.
pub struct SyncStart {
    channel: Box<dyn PeerChannel>,
    peer_device_id: DeviceId,

    /// The scope the initiator announced. If merging the responder's registry
    /// widens it, the initiator announces the revision before any blob moves.
    sent_scope: BTreeSet<VaultId>,
}

impl SyncStart {
    pub fn peer_device_id(&self) -> DeviceId {
        self.peer_device_id
    }
}

/// An initiator that has sent its offers and is waiting for the responder's.
pub struct SyncSent {
    channel: Box<dyn PeerChannel>,
    peer_device_id: DeviceId,

    /// Vaults this Device is willing to accept: its Sync Set over the merged
    /// registry. A responder cannot push anything outside it.
    accepting: BTreeSet<VaultId>,
}

/// A responder that has received the initiator's Scope and sent its own.
pub struct SyncAccepted {
    channel: Box<dyn PeerChannel>,
    peer_device_id: DeviceId,
    intersection: BTreeSet<VaultId>,
}

/// One sync engine over the single network seam.
pub struct SyncEngine<'a> {
    network: &'a dyn PeerNetwork,
    vault_manager: &'a mut VaultManager,
    user_manager: &'a UserManager,
    credentials: SyncCredentials<'a>,
    sync_set: SyncSet,
    cancel: SyncCancel,
}

impl<'a> SyncEngine<'a> {
    pub fn new(
        network: &'a dyn PeerNetwork,
        vault_manager: &'a mut VaultManager,
        user_manager: &'a UserManager,
        sync_set: SyncSet,
        credentials: SyncCredentials<'a>,
    ) -> Self {
        Self {
            network,
            vault_manager,
            user_manager,
            credentials,
            sync_set,
            cancel: SyncCancel::new(),
        }
    }

    /// Attach the cancel token claimed from a [`SyncControl`].
    pub fn with_cancel(mut self, cancel: SyncCancel) -> Self {
        self.cancel = cancel;
        self
    }

    // ============ Initiator ============

    /// Connect and send this Device's Scope. Nothing has been written yet.
    pub fn start_with_peer(
        &mut self,
        peer: &DiscoveredPeer,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<SyncStart, VaultError> {
        let user_id = self.signed_in_user()?;
        let shared = self.user_manager.get_shared_profile(&user_id)?;

        // Pairing is the only grant of access. Discovery, or a channel that
        // merely exists, is never enough: an advertiser this Device has not
        // paired with is refused before a socket is opened.
        self.require_admitted_peer(peer.device_id(), &shared.devices)?;
        self.check_cancelled()?;

        let local_scope = self.sync_set.ids(&shared.vaults);
        let local_scope_ids: Vec<VaultId> = local_scope.iter().copied().collect();

        let mut channel = self
            .network
            .connect(peer)
            .map_err(Self::map_connect_error)?;

        // The channel must really lead to the Device that was asked for.
        if channel.peer_device_id() != peer.device_id() {
            channel.close();
            return Err(VaultError::SyncWrongPeer);
        }

        // A dirty vault must travel as it is now, not as it was last saved.
        self.vault_manager.flush_all()?;

        self.check_cancelled()?;
        channel
            .send(SyncMessage::Scope(ScopeState::new(
                local_scope_ids.clone(),
                shared.registries(),
            )))
            .map_err(Self::map_channel_error)?;

        progress(SyncProgress::Started {
            peer_device_id: peer.device_id(),
            vault_ids: local_scope_ids,
        });

        Ok(SyncStart {
            channel,
            peer_device_id: peer.device_id(),
            sent_scope: local_scope,
        })
    }

    /// Receive the peer's Scope, merge the shared registries, and send this
    /// Device's vault offers for the intersection.
    pub fn send_offers(
        &mut self,
        mut start: SyncStart,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<SyncSent, VaultError> {
        let remote_scope = self.receive_scope(&mut *start.channel)?;
        let merged_registry = self.merge_registries(&remote_scope.registries)?;

        self.require_admitted_peer(start.peer_device_id, &merged_registry.devices)?;

        // Recompute the local scope from the *merged* registry. The peer may
        // have told us about a Vault it created since the last sync; taking
        // this Device's default Sync Set before the merge would leave that
        // Vault until the following round.
        let local_scope = self.sync_set.ids(&merged_registry.vaults);
        let intersection = self.intersection(
            &local_scope,
            &remote_scope.vault_ids,
            &merged_registry.vaults,
        );

        if !local_scope.is_subset(&start.sent_scope) {
            // The responder decides what to offer from the scope we
            // announced, which predates the registry it just gave us.
            // Announce the wider scope so it can include those Vaults in the
            // same round.
            let revised: Vec<VaultId> = local_scope.iter().copied().collect();
            start
                .channel
                .send(SyncMessage::Scope(ScopeState::new(
                    revised,
                    merged_registry.clone(),
                )))
                .map_err(Self::map_channel_error)?;
        }

        let agreed: Vec<VaultId> = intersection.iter().copied().collect();
        progress(SyncProgress::ScopeAgreed {
            peer_device_id: start.peer_device_id,
            vault_ids: agreed,
        });

        for vault_id in &intersection {
            self.send_vault(start.channel.as_mut(), *vault_id, progress)?;
        }
        self.check_cancelled()?;
        start
            .channel
            .send(SyncMessage::Complete)
            .map_err(Self::map_channel_error)?;

        Ok(SyncSent {
            channel: start.channel,
            peer_device_id: start.peer_device_id,
            // What this Device is willing to accept from the responder: its
            // whole Sync Set, not only the part the responder already knew
            // about. The responder still sends only Vaults it holds and has
            // in its own Sync Set.
            accepting: local_scope,
        })
    }

    /// Receive the responder's offers, merge them, and finish.
    pub fn collect(
        &mut self,
        mut sent: SyncSent,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<SyncOutcome, VaultError> {
        let session_id = Uuid::new_v4();
        let mut outcome = SyncOutcome::report_only(session_id, sent.peer_device_id);

        loop {
            self.require_admitted_peer(sent.peer_device_id, &self.local_devices()?)?;
            match self.receive_frame(sent.channel.as_mut())? {
                SyncMessage::Vault { vault_id, blob } => {
                    self.integrator_check_scope(&sent.accepting, vault_id)?;
                    self.integrate(vault_id, &blob, &mut outcome, progress)?;
                }
                SyncMessage::Complete => break,
                other => {
                    return Err(VaultError::SyncRefused(format!(
                        "unexpected frame while receiving vaults: {other:?}"
                    )))
                }
            }
        }

        self.finish_outcome(outcome, progress)
    }

    // ============ Responder ============

    /// Take the next inbound channel and answer the initiator's Scope.
    /// Returns `Ok(None)` when no connection is waiting.
    pub fn accept(
        &mut self,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<Option<SyncAccepted>, VaultError> {
        let user_id = self.signed_in_user()?;
        let Some(mut channel) = self.network.accept().map_err(Self::map_connect_error)? else {
            return Ok(None);
        };

        let peer_device_id = channel.peer_device_id();

        // Fail closed before reading anything: an inbound channel from a
        // Device this User never paired with gets no registry state and no
        // vault ids. Run this again after the merge, because the peer's own
        // registry may revoke it.
        if let Err(error) = self.require_admitted_peer(
            peer_device_id,
            &self.user_manager.get_shared_profile(&user_id)?.devices,
        ) {
            channel.close();
            return Err(error);
        }

        let remote_scope = self.receive_scope(&mut *channel)?;
        let merged_registry = self.merge_registries(&remote_scope.registries)?;
        if let Err(error) = self.require_admitted_peer(peer_device_id, &merged_registry.devices) {
            channel.close();
            return Err(error);
        }

        self.vault_manager.flush_all()?;
        self.check_cancelled()?;

        let local_scope = self.sync_set.ids(&merged_registry.vaults);
        let local_ids: Vec<VaultId> = local_scope.iter().copied().collect();
        channel
            .send(SyncMessage::Scope(ScopeState::new(
                local_ids,
                merged_registry.clone(),
            )))
            .map_err(Self::map_channel_error)?;

        let intersection = self.intersection(
            &local_scope,
            &remote_scope.vault_ids,
            &merged_registry.vaults,
        );

        let agreed: Vec<VaultId> = intersection.iter().copied().collect();
        progress(SyncProgress::ScopeAgreed {
            peer_device_id,
            vault_ids: agreed,
        });

        Ok(Some(SyncAccepted {
            channel,
            peer_device_id,
            intersection,
        }))
    }

    /// Receive the initiator's offers, merge them, then send this Device's own
    /// offers for the intersection.
    pub fn receive_offers(
        &mut self,
        mut accepted: SyncAccepted,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<SyncOutcome, VaultError> {
        let session_id = Uuid::new_v4();
        let mut outcome = SyncOutcome::report_only(session_id, accepted.peer_device_id);

        // The peer may have been revoked while the channel was open, and a
        // revocation that arrived with the peer's registry was merged during
        // `accept`. Stop before any vault content is sent or integrated.
        self.require_admitted_peer(accepted.peer_device_id, &self.local_devices()?)?;

        // The initiator may announce a revised scope after merging our
        // registry (it has learned about Vaults we hold that it did not know).
        // Recompute the intersection before deciding what to offer, otherwise
        // a Vault created here since the last sync would wait a round.
        let mut intersection = accepted.intersection.clone();
        let mut current = self.receive_frame(accepted.channel.as_mut())?;
        while let SyncMessage::Scope(scope) = &current {
            let merged_registry = self.merge_registries(&scope.registries)?;
            self.require_admitted_peer(accepted.peer_device_id, &merged_registry.devices)?;
            intersection = self.intersection(
                &self.sync_set.ids(&merged_registry.vaults),
                &scope.vault_ids,
                &merged_registry.vaults,
            );
            current = self.receive_frame(accepted.channel.as_mut())?;
        }

        // Capture this Device's copies *before* merging the initiator's
        // offers. Sending the post-merge copy back would make it look
        // causally later than the initiator's own write (it has absorbed the
        // initiator's clock), silently hiding the very conflict the two
        // original copies describe.
        let own_offers: Vec<(VaultId, Vec<u8>)> = intersection
            .iter()
            .filter(|vault_id| self.vault_manager.has_vault_file(**vault_id))
            .map(|vault_id| {
                Ok((
                    *vault_id,
                    self.vault_manager.read_encrypted_vault(*vault_id)?,
                ))
            })
            .collect::<Result<_, VaultError>>()?;

        loop {
            self.require_admitted_peer(accepted.peer_device_id, &self.local_devices()?)?;
            match current {
                SyncMessage::Vault { vault_id, blob } => {
                    self.integrator_check_scope(&intersection, vault_id)?;
                    self.integrate(vault_id, &blob, &mut outcome, progress)?;
                }
                SyncMessage::Complete => break,
                other => {
                    return Err(VaultError::SyncRefused(format!(
                        "unexpected frame while receiving vaults: {other:?}"
                    )))
                }
            }
            current = self.receive_frame(accepted.channel.as_mut())?;
        }

        // Responder replies with the pre-merge copies captured above. Both
        // Devices therefore merge the same two original states, compute the
        // same conflicts, and keep their own contested values.
        for (vault_id, bytes) in own_offers {
            self.check_cancelled()?;
            progress(SyncProgress::SendingVault {
                vault_id,
                bytes: bytes.len(),
            });
            accepted
                .channel
                .send(SyncMessage::Vault {
                    vault_id,
                    blob: EncryptedVaultBlob::from_encrypted_bytes(bytes),
                })
                .map_err(Self::map_channel_error)?;
        }
        self.check_cancelled()?;
        accepted
            .channel
            .send(SyncMessage::Complete)
            .map_err(Self::map_channel_error)?;

        self.finish_outcome(outcome, progress)
    }

    // ============ Shared steps ============

    fn signed_in_user(&self) -> Result<UserId, VaultError> {
        let user_id = self
            .user_manager
            .active_user_id
            .ok_or(VaultError::SyncNotLoggedIn)?;

        if self.credentials.secret_key.len() != 32 || self.credentials.master_password.is_empty() {
            return Err(VaultError::SyncNotLoggedIn);
        }

        Ok(user_id)
    }

    /// The Device Registry this Device has right now.
    fn local_devices(&self) -> Result<crate::DeviceRegistry, VaultError> {
        let user_id = self
            .user_manager
            .active_user_id
            .ok_or(VaultError::SyncNotLoggedIn)?;
        Ok(self.user_manager.get_shared_profile(&user_id)?.devices)
    }

    /// Refuse a peer that this User has not admitted, or has revoked.
    ///
    /// An unknown Device is not "merely unrevoked": `is_revoked` is false for
    /// it, so checking revocation alone would let any advertiser in.
    fn require_admitted_peer(
        &self,
        peer_device_id: DeviceId,
        devices: &crate::DeviceRegistry,
    ) -> Result<(), VaultError> {
        match devices.get(&peer_device_id) {
            Some(record) if record.is_revoked() => {
                Err(VaultError::SyncPeerRevoked(peer_device_id.to_string()))
            }
            Some(_) => Ok(()),
            None => Err(VaultError::SyncPeerNotPaired(peer_device_id.to_string())),
        }
    }

    fn check_cancelled(&self) -> Result<(), VaultError> {
        if self.cancel.is_cancelled() {
            Err(VaultError::SyncCancelled)
        } else {
            Ok(())
        }
    }

    /// Receive one frame, refusing to wait forever and honouring cancellation.
    ///
    /// A transport may only observe the cancel token at the next frame
    /// boundary, so a cancel that arrives while a frame is in flight is
    /// reported as a cancellation rather than as a timeout. Cancellation
    /// latency is therefore bounded by [`SYNC_FRAME_TIMEOUT`].
    fn receive_frame(&self, channel: &mut dyn PeerChannel) -> Result<SyncMessage, VaultError> {
        self.check_cancelled()?;
        match channel.receive_timeout(SYNC_FRAME_TIMEOUT) {
            Ok(message) => Ok(message),
            Err(error) => {
                self.check_cancelled()?;
                Err(Self::map_channel_error(error))
            }
        }
    }

    fn receive_scope(&self, channel: &mut dyn PeerChannel) -> Result<ScopeState, VaultError> {
        match self.receive_frame(channel)? {
            SyncMessage::Scope(scope) => Ok(scope),
            other => Err(VaultError::SyncRefused(format!(
                "expected the peer's scope, got {other:?}"
            ))),
        }
    }

    /// Merge the peer's shared registries into ours. Revocation wins, so a
    /// peer cannot un-revoke a Device by presenting an older copy.
    ///
    /// A Vault Tombstone that arrives here is applied immediately: this
    /// Device's live copy becomes local trash. Doing it inside the registry
    /// merge is what makes "re-enable a Vault that was deleted while it was
    /// out of the Sync Set" apply the pending tombstone without a second
    /// round trip.
    fn merge_registries(&mut self, remote: &SharedRegistries) -> Result<SharedRegistries, VaultError> {
        let user_id = self
            .user_manager
            .active_user_id
            .ok_or(VaultError::SyncNotLoggedIn)?;
        let mut shared = self.user_manager.get_shared_profile(&user_id)?;

        shared.devices = shared.devices.merge(&remote.devices)?;
        shared.vaults = shared.vaults.merge(&remote.vaults);

        if shared.devices.is_revoked(&self.vault_manager.device_id()) {
            return Err(VaultError::SyncThisDeviceRevoked(
                self.vault_manager.device_id().to_string(),
            ));
        }

        let tombstoned: Vec<VaultId> = shared
            .vaults
            .tombstones()
            .map(|tombstone| tombstone.vault_id)
            .collect();

        self.user_manager.save_shared_profile(&shared)?;

        for vault_id in tombstoned {
            if self.vault_manager.has_vault_file(vault_id) {
                self.vault_manager.move_to_trash(vault_id)?;
            }
        }

        Ok(shared.registries())
    }

    /// Registry ∩ both Sync Sets. Nothing outside this set is offered or
    /// requested.
    fn intersection(
        &self,
        local_scope: &BTreeSet<VaultId>,
        remote_scope: &[VaultId],
        registry: &VaultRegistry,
    ) -> BTreeSet<VaultId> {
        let remote: BTreeSet<VaultId> = remote_scope.iter().copied().collect();
        local_scope
            .intersection(&remote)
            .filter(|vault_id| registry.contains(vault_id))
            .copied()
            .collect()
    }

    fn integrator_check_scope(
        &self,
        intersection: &BTreeSet<VaultId>,
        vault_id: VaultId,
    ) -> Result<(), VaultError> {
        if intersection.contains(&vault_id) {
            Ok(())
        } else {
            Err(VaultError::SyncRefused(format!(
                "peer offered Vault {vault_id} outside the agreed Sync Set intersection"
            )))
        }
    }

    fn send_vault(
        &self,
        channel: &mut dyn PeerChannel,
        vault_id: VaultId,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<(), VaultError> {
        self.check_cancelled()?;
        if !self.vault_manager.has_vault_file(vault_id) {
            // The registry can name a Vault this Device does not hold. That
            // is not an error: the peer may hold it and send it to us.
            return Ok(());
        }

        let bytes = self.vault_manager.read_encrypted_vault(vault_id)?;
        progress(SyncProgress::SendingVault {
            vault_id,
            bytes: bytes.len(),
        });
        channel
            .send(SyncMessage::Vault {
                vault_id,
                blob: EncryptedVaultBlob::from_encrypted_bytes(bytes),
            })
            .map_err(Self::map_channel_error)
    }

    fn integrate(
        &mut self,
        vault_id: VaultId,
        blob: &EncryptedVaultBlob,
        outcome: &mut SyncOutcome,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<(), VaultError> {
        progress(SyncProgress::ReceivingVault {
            vault_id,
            bytes: blob.len(),
        });

        let remote = self.vault_manager.decrypt_vault_bytes(
            blob.as_bytes(),
            self.credentials.master_password,
            self.credentials.secret_key,
        )?;

        if remote.id != vault_id {
            return Err(VaultError::SyncVaultIdMismatch {
                local: vault_id.to_string(),
                remote: remote.id.to_string(),
            });
        }

        // The transport handshake compares schema versions, but a blob can
        // arrive over a channel that did not (or claim otherwise). Never
        // integrate a schema this build cannot honestly read: a newer one
        // would lose fields on merge, an older one is unmigrated.
        if remote.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(VaultError::SyncSchemaMismatch {
                local: CURRENT_SCHEMA_VERSION,
                remote: remote.schema_version,
            });
        }

        if !self.vault_manager.has_vault_file(vault_id) {
            // First copy of this Vault on this Device.
            self.vault_manager.install_vault(
                remote,
                self.credentials.master_password,
                self.credentials.secret_key,
            )?;
            outcome.report.vault_ids.push(vault_id);
            progress(SyncProgress::VaultMerged { vault_id });
            return Ok(());
        }

        self.vault_manager.unlock_vault(
            vault_id,
            self.credentials.master_password,
            self.credentials.secret_key,
        )?;
        let local = self.vault_manager.get_vault(vault_id)?.clone();

        // MergeError::VaultIdMismatch is a distinct failure: two copies that
        // name different Vaults are never relabelled into one.
        match crate::merge::merge_vaults(&local, &remote).map_err(|err| match err {
            MergeError::VaultIdMismatch { left, right } => VaultError::SyncVaultIdMismatch {
                local: left.to_string(),
                remote: right.to_string(),
            },
        })? {
            MergeOutcome::Merged(merged) => {
                self.vault_manager.replace_vault(merged)?;
            }
            MergeOutcome::Conflicts(report) => {
                // An Item's kind is not a versioned field, so a disagreement
                // about it cannot be resolved the way a field Conflict can.
                // No local operation may change an Item's kind, so this is a
                // malformed (or tampered) copy rather than an edit a human
                // made: fail closed instead of surfacing a prompt that could
                // never drain the session.
                if report
                    .conflicts
                    .iter()
                    .any(|conflict| conflict.field_key == "kind")
                {
                    return Err(VaultError::SyncRefused(format!(
                        "Vault {vault_id}: the peer's copy disagrees about an Item's kind, which cannot be merged"
                    )));
                }

                outcome.pending.extend(self.capture_conflicts(
                    vault_id,
                    &local,
                    &remote,
                    &report.conflicts,
                ));
                let safe = crate::merge::keep_local_for_conflicts(&local, &report);
                self.vault_manager.replace_vault(safe)?;

                progress(SyncProgress::VaultConflicted {
                    vault_id,
                    conflicts: report.conflicts.len(),
                });
            }
        }

        outcome.report.vault_ids.push(vault_id);
        progress(SyncProgress::VaultMerged { vault_id });
        Ok(())
    }

    /// Capture both candidate values before they are discarded. Secret values
    /// stay in this backend-only structure; the report only carries the safe
    /// `Conflict`.
    fn capture_conflicts(
        &self,
        vault_id: VaultId,
        local: &Vault,
        remote: &Vault,
        conflicts: &[Conflict],
    ) -> Vec<PendingConflict> {
        conflicts
            .iter()
            .map(|conflict| PendingConflict {
                vault_id,
                scope: conflict.scope.clone(),
                field_key: conflict.field_key.clone(),
                is_secret: conflict.is_secret,
                local: capture_value(
                    local,
                    &conflict.scope,
                    &conflict.field_key,
                    conflict.is_secret,
                ),
                remote: capture_value(
                    remote,
                    &conflict.scope,
                    &conflict.field_key,
                    conflict.is_secret,
                ),
                local_version: capture_field_version(local, &conflict.scope, &conflict.field_key),
                sides: conflict.sides.clone(),
            })
            .collect()
    }

    fn finish_outcome(
        &mut self,
        mut outcome: SyncOutcome,
        progress: &mut dyn FnMut(SyncProgress),
    ) -> Result<SyncOutcome, VaultError> {
        outcome.report.vault_ids.sort();
        outcome.report.vault_ids.dedup();
        outcome.report.conflicts = outcome
            .pending
            .iter()
            .map(|pending| SyncConflict {
                vault_id: pending.vault_id,
                conflict: Conflict {
                    scope: pending.scope.clone(),
                    field_key: pending.field_key.clone(),
                    is_secret: pending.is_secret,
                    sides: pending.sides.clone(),
                },
            })
            .collect();
        outcome.report.duplicate_notes = self.duplicate_identity_notes();

        progress(SyncProgress::Complete {
            vaults: outcome.report.vault_ids.len(),
            conflicts: outcome.report.conflicts.len(),
        });

        Ok(outcome)
    }

    /// Same item identity live in two local Vaults. This is the move
    /// collision that whole-file, per-Vault merge cannot resolve by itself.
    ///
    /// This is a surviving limitation, not a promise of convergence: a merge
    /// sees one Vault at a time, so two Devices moving the same identity to
    /// two different destinations concurrently can leave a live copy in each.
    /// #47 picks the destination deterministically inside the source Vault;
    /// reconciling the two destination Vaults needs cross-Vault knowledge that
    /// this engine does not have. The note exists so nothing is silently
    /// discarded, not so the caller may claim the two Vaults agree.
    fn duplicate_identity_notes(&self) -> Vec<DuplicateIdentityNote> {
        let mut by_item: BTreeMap<ItemId, Vec<VaultId>> = BTreeMap::new();

        for safe in self.vault_manager.get_unlocked_vaults() {
            let Ok(vault) = self.vault_manager.get_vault(safe.id) else {
                continue;
            };
            for item in &vault.items {
                // `is_tombstone()` is also true for move records, which are
                // content-free. The live filter is explicit instead.
                if item.deleted || item.moved_to.is_some() || item.kind.is_none() {
                    continue;
                }
                by_item.entry(item.id).or_default().push(safe.id);
            }
        }

        by_item
            .into_iter()
            .filter(|(_, vault_ids)| vault_ids.len() > 1)
            .map(|(item_id, mut vault_ids)| {
                vault_ids.sort();
                vault_ids.dedup();
                DuplicateIdentityNote { item_id, vault_ids }
            })
            .filter(|note| note.vault_ids.len() > 1)
            .collect()
    }

    fn map_connect_error(err: NetworkError) -> VaultError {
        match err {
            NetworkError::PeerUnreachable(peer) => VaultError::SyncPeerUnreachable(peer),
            NetworkError::AuthenticationFailed => VaultError::SyncAuthenticationFailed,
            NetworkError::ProtocolMismatch { local, remote } => {
                VaultError::SyncProtocolMismatch { local, remote }
            }
            NetworkError::SchemaMismatch { local, remote } => {
                VaultError::SyncSchemaMismatch { local, remote }
            }
            NetworkError::DiscoveryFailed(detail) => VaultError::SyncDiscoveryFailed(detail),
            NetworkError::TransportUnavailable(detail) => {
                VaultError::SyncTransportUnavailable(detail)
            }
            NetworkError::ConnectionClosed => {
                VaultError::SyncPeerUnreachable("connection closed".into())
            }
            NetworkError::Timeout => VaultError::SyncTimeout("the peer did not answer".into()),
            NetworkError::MalformedFrame(detail) => VaultError::SyncRefused(detail),
        }
    }

    fn map_channel_error(err: NetworkError) -> VaultError {
        match err {
            NetworkError::ConnectionClosed => {
                VaultError::SyncInterrupted("the peer channel closed".into())
            }
            NetworkError::Timeout => VaultError::SyncTimeout(
                "the peer did not answer within the frame timeout".into(),
            ),
            NetworkError::PeerUnreachable(peer) => VaultError::SyncPeerUnreachable(peer),
            NetworkError::AuthenticationFailed => VaultError::SyncAuthenticationFailed,
            NetworkError::ProtocolMismatch { local, remote } => {
                VaultError::SyncProtocolMismatch { local, remote }
            }
            NetworkError::SchemaMismatch { local, remote } => {
                VaultError::SyncSchemaMismatch { local, remote }
            }
            NetworkError::DiscoveryFailed(detail) => VaultError::SyncDiscoveryFailed(detail),
            NetworkError::TransportUnavailable(detail) => {
                VaultError::SyncTransportUnavailable(detail)
            }
            NetworkError::MalformedFrame(detail) => VaultError::SyncRefused(detail),
        }
    }
}

/// The local field's last-writer version, captured with the candidates so a
/// later resolution can tell whether the field moved underneath it.
fn capture_field_version(
    vault: &Vault,
    scope: &ConflictScope,
    field_key: &str,
) -> Option<FieldVersion> {
    match scope {
        ConflictScope::Item { item_id, .. } => vault
            .items
            .iter()
            .find(|item| item.id == *item_id && !item.is_tombstone())
            .and_then(|item| item.field_clocks.get(field_key).copied()),
        ConflictScope::Vault => match field_key {
            "name" => Some(vault.name_clock),
            "color" => Some(vault.color_clock),
            _ => None,
        },
    }
}

/// Read one candidate value from a vault copy.
fn capture_value(
    vault: &Vault,
    scope: &ConflictScope,
    field_key: &str,
    is_secret: bool,
) -> Option<FieldValue> {
    match scope {
        ConflictScope::Item { item_id, .. } => {
            let item = vault
                .items
                .iter()
                .find(|item| item.id == *item_id && !item.deleted)?;
            if is_secret {
                vault
                    .secrets
                    .iter()
                    .find(|entry| entry.item_id == *item_id)
                    .and_then(|entry| entry.get(field_key))
                    .map(|value| FieldValue::Secret(value.to_string()))
            } else {
                crate::merge::read_item_field(item, field_key)
            }
        }
        ConflictScope::Vault => match field_key {
            "name" => Some(FieldValue::Plain(vault.name.clone())),
            "color" => Some(FieldValue::Plain(vault.color.clone())),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;

    use super::*;
    use crate::network::{
        InMemoryDeviceConfig, InMemoryHub, InMemoryNetwork, PeerChannel, SyncMessage,
    };
    use crate::{AccountData, NewItemData, PairingCode, SecretInput, CURRENT_SCHEMA_VERSION};

    const MASTER_PW: &str = "Correct-Horse-Battery-Staple-9";
    const SCHEMA_VERSION: u32 = 3;

    fn setup_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lockr_sync_{tag}_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    struct Device {
        dir: PathBuf,
        user: UserManager,
        vaults: VaultManager,
        net: InMemoryNetwork,
        secret_key: Vec<u8>,
        user_id: UserId,
        sync: SyncStateStore,
    }

    impl Device {
        fn scope(&self) -> SyncSet {
            self.sync.scope(&self.user_id)
        }
    }

    struct Fixture {
        _hub: Arc<InMemoryHub>,
        a: Device,
        b: Device,
        vault_id: VaultId,
        item_id: ItemId,
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.a.dir).ok();
            std::fs::remove_dir_all(&self.b.dir).ok();
        }
    }

    fn live_item<'a>(
        device: &'a Device,
        vault_id: VaultId,
        item_id: ItemId,
    ) -> &'a crate::VaultItem {
        device
            .vaults
            .get_vault(vault_id)
            .unwrap()
            .items
            .iter()
            .find(|item| item.id == item_id)
            .expect("the item is retained")
    }

    fn account_data(item: &crate::VaultItem) -> &AccountData {
        match item.kind.as_ref().expect("a live item has a kind") {
            crate::ItemKind::Account(data) => data,
            other => panic!("expected an account, got {other:?}"),
        }
    }

    impl Fixture {
        fn new() -> Self {
            let a_dir = setup_dir("a");
            let b_dir = setup_dir("b");

            let mut source_user = UserManager::new(a_dir.clone()).unwrap();
            let user = source_user
                .register("Alice".to_string(), MASTER_PW)
                .unwrap();
            let source_key = source_user.login(&user.id, MASTER_PW).unwrap();

            let mut source_vaults = VaultManager::new(a_dir.clone()).unwrap();
            let vault = source_vaults
                .create_vault(
                    "Personal".to_string(),
                    "#6240BF".to_string(),
                    MASTER_PW,
                    &source_key,
                )
                .unwrap();
            source_user.add_vault_to_user(vault.id).unwrap();
            let item = source_vaults
                .add_item(
                    vault.id,
                    Some("Mail".to_string()),
                    None,
                    NewItemData::Account(AccountData {
                        username: "alice".to_string(),
                        email: None,
                    }),
                    Some(vec![SecretInput {
                        key: "password".to_string(),
                        value: "hunter2".to_string(),
                    }]),
                )
                .unwrap();
            let item_id = match item {
                crate::SafeItem::Account(account) => account.id,
                _ => panic!("expected an account"),
            };

            let mut joiner_user = UserManager::new(b_dir.clone()).unwrap();
            let joiner_vaults = VaultManager::new(b_dir.clone()).unwrap();

            let hub = InMemoryHub::new();
            let source_net = hub.join(
                InMemoryDeviceConfig::new(source_user.device_id(), SCHEMA_VERSION)
                    .paired_with([joiner_user.device_id()]),
            );
            let joiner_net = hub.join(
                InMemoryDeviceConfig::new(joiner_user.device_id(), SCHEMA_VERSION)
                    .paired_with([source_user.device_id()]),
            );
            source_net.start_advertising().unwrap();

            // Pair the two Devices through the real pairing protocol, so the
            // registries and the joining Device's active session are honest.
            let now = Utc::now();
            let offer = source_user
                .start_pairing(&source_key, SCHEMA_VERSION, now)
                .unwrap();
            let code = PairingCode::qr(offer.qr_payload.clone());
            let claim = code.claim().unwrap();
            source_net.begin_pairing(&claim).unwrap();

            let join = joiner_user
                .join_pairing(&code, SCHEMA_VERSION, now)
                .unwrap();
            let peer = joiner_net
                .discover()
                .unwrap()
                .into_iter()
                .find(|peer| peer.device_id() == source_user.device_id())
                .expect("the source is advertising");
            let mut initiator = joiner_net.connect_for_pairing(&peer, &claim).unwrap();
            let mut responder = source_net
                .accept_for_pairing(&claim)
                .unwrap()
                .expect("the join is waiting");

            initiator.send(SyncMessage::Pairing(join)).unwrap();
            let request = match responder.receive().unwrap() {
                SyncMessage::Pairing(message) => message,
                other => panic!("expected a pairing request, got {other:?}"),
            };
            let prompt = source_user.receive_pairing_request(request, now).unwrap();
            let transfer = source_user.approve_pairing(prompt.device_id, now).unwrap();
            responder.send(SyncMessage::Pairing(transfer)).unwrap();
            let transfer = match initiator.receive().unwrap() {
                SyncMessage::Pairing(message) => message,
                other => panic!("expected a pairing transfer, got {other:?}"),
            };
            joiner_user
                .complete_pairing(transfer, MASTER_PW, now)
                .unwrap();
            source_net.end_pairing().unwrap();

            let joiner_key = joiner_user.login(&user.id, MASTER_PW).unwrap();

            // The source already held the vault when it paired, so the
            // joiner's registry names it. Copy the encrypted file and unlock.
            std::fs::create_dir_all(b_dir.join("vaults")).unwrap();
            std::fs::copy(
                a_dir.join("vaults").join(format!("{}.vault", vault.id)),
                b_dir.join("vaults").join(format!("{}.vault", vault.id)),
            )
            .unwrap();
            let mut joiner_vaults = joiner_vaults;
            joiner_vaults
                .unlock_vault(vault.id, MASTER_PW, &joiner_key)
                .unwrap();
            joiner_net.start_advertising().unwrap();

            let a_sync = SyncStateStore::new(&a_dir).unwrap();
            let b_sync = SyncStateStore::new(&b_dir).unwrap();

            Self {
                _hub: hub,
                a: Device {
                    dir: a_dir,
                    user: source_user,
                    vaults: source_vaults,
                    net: source_net,
                    secret_key: source_key,
                    user_id: user.id,
                    sync: a_sync,
                },
                b: Device {
                    dir: b_dir,
                    user: joiner_user,
                    vaults: joiner_vaults,
                    net: joiner_net,
                    secret_key: joiner_key,
                    user_id: user.id,
                    sync: b_sync,
                },
                vault_id: vault.id,
                item_id,
            }
        }

        fn sync(&mut self) -> (SyncOutcome, SyncOutcome) {
            let (a, b) = self.sync_attempt();
            (
                a.expect("Device A completed the sync"),
                b.expect("Device B completed the sync"),
            )
        }

        /// The same exchange, exposing each side's result. Used by tests that
        /// expect the sync to refuse part-way through.
        fn sync_attempt(
            &mut self,
        ) -> (Result<SyncOutcome, VaultError>, Result<SyncOutcome, VaultError>) {
            let peer = self
                .a
                .net
                .discover()
                .unwrap()
                .into_iter()
                .find(|peer| peer.device_id() == self.b.user.device_id())
                .expect("b is advertising");

            let mut noop = |_: SyncProgress| {};
            let a_scope = self.a.scope();
            let a_credentials = SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &self.a.secret_key,
            };
            let mut engine_a = SyncEngine::new(
                &self.a.net,
                &mut self.a.vaults,
                &self.a.user,
                a_scope,
                a_credentials,
            );
            let start = engine_a.start_with_peer(&peer, &mut noop).unwrap();

            let b_scope = self.b.scope();
            let b_credentials = SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &self.b.secret_key,
            };
            let mut engine_b = SyncEngine::new(
                &self.b.net,
                &mut self.b.vaults,
                &self.b.user,
                b_scope,
                b_credentials,
            );
            let accepted = engine_b
                .accept(&mut noop)
                .unwrap()
                .expect("the initiator is waiting");

            let sent = engine_a.send_offers(start, &mut noop).unwrap();
            let outcome_b = engine_b.receive_offers(accepted, &mut noop);
            let outcome_a = engine_a.collect(sent, &mut noop);

            (outcome_a, outcome_b)
        }
    }

    fn pending_for<'a>(outcome: &'a SyncOutcome, field_key: &str) -> &'a PendingConflict {
        outcome
            .pending
            .iter()
            .find(|pending| pending.field_key == field_key)
            .unwrap_or_else(|| panic!("expected a pending conflict for {field_key}"))
    }

    #[test]
    fn test_sync_set_defaults_to_every_registry_vault_and_survives_a_restart() {
        let dir = setup_dir("state");
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let user = Uuid::new_v4();

        let mut registry = VaultRegistry::new();
        registry.insert(first);
        registry.insert(second);

        let mut store = SyncStateStore::new(&dir).unwrap();
        assert_eq!(
            store.sync_set(&user, &registry),
            BTreeSet::from([first, second]),
            "a freshly paired Device carries every Vault the registry knows"
        );

        store
            .set_vault_syncing(&user, first, false, &registry)
            .unwrap();
        assert_eq!(store.sync_set(&user, &registry), BTreeSet::from([second]));

        let at = Utc::now();
        store.record_sync(&user, Uuid::new_v4(), at).unwrap();
        drop(store);

        let reloaded = SyncStateStore::new(&dir).unwrap();
        assert_eq!(
            reloaded.sync_set(&user, &registry),
            BTreeSet::from([second]),
            "the Sync Set is device-local state and survives a restart"
        );
        assert!(reloaded.scope(&user).is_explicit());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_two_devices_converge_to_identical_vault_content_including_secrets_and_tombstones() {
        let mut fixture = Fixture::new();
        fixture.sync();

        assert_eq!(
            fixture.a.vaults.get_vault(fixture.vault_id).unwrap(),
            fixture.b.vaults.get_vault(fixture.vault_id).unwrap()
        );
        assert_eq!(
            fixture
                .b
                .vaults
                .get_item_secret(fixture.vault_id, fixture.item_id, "password")
                .unwrap()
                .as_deref(),
            Some("hunter2")
        );

        // A change made on B travels to A.
        let note = fixture
            .b
            .vaults
            .add_item(
                fixture.vault_id,
                Some("Recovery".to_string()),
                None,
                NewItemData::Note(crate::NoteData {
                    title: Some("Codes".to_string()),
                    body: "one two three".to_string(),
                }),
                None,
            )
            .unwrap();
        let note_id = match note {
            crate::SafeItem::Note(note) => note.id,
            _ => panic!("expected a note"),
        };

        fixture.sync();
        assert_eq!(
            fixture.a.vaults.get_vault(fixture.vault_id).unwrap(),
            fixture.b.vaults.get_vault(fixture.vault_id).unwrap()
        );

        // A deletion becomes a Tombstone on the other side, not a resurrection.
        fixture
            .b
            .vaults
            .delete_item(fixture.vault_id, fixture.item_id)
            .unwrap();
        fixture.sync();

        let on_a = live_item(&fixture.a, fixture.vault_id, fixture.item_id);
        assert!(on_a.deleted, "the deletion must travel as a Tombstone");
        assert!(on_a.kind.is_none());
        assert_eq!(
            fixture.a.vaults.get_vault(fixture.vault_id).unwrap(),
            fixture.b.vaults.get_vault(fixture.vault_id).unwrap()
        );
        assert!(
            !fixture
                .a
                .vaults
                .get_all_items(crate::ItemFilter::default())
                .unwrap()
                .iter()
                .any(|item| match item {
                    crate::SafeItem::Account(account) => account.id == fixture.item_id,
                    _ => false,
                }),
            "the deleted item must not be listed"
        );
        assert!(
            fixture
                .a
                .vaults
                .get_all_items(crate::ItemFilter::default())
                .unwrap()
                .iter()
                .any(|item| match item {
                    crate::SafeItem::Note(note) => note.id == note_id,
                    _ => false,
                }),
            "the unrelated item remains listed"
        );

        // A completed transfer is idempotent: retrying does not double-apply.
        let before = fixture
            .a
            .vaults
            .get_vault(fixture.vault_id)
            .unwrap()
            .clone();
        fixture.sync();
        assert_eq!(
            fixture.a.vaults.get_vault(fixture.vault_id).unwrap(),
            &before,
            "retrying a completed sync must not change the vault"
        );
    }

    #[test]
    fn test_a_vault_outside_the_remote_sync_set_is_neither_sent_nor_requested() {
        let mut fixture = Fixture::new();

        // A creates a second Vault after pairing. B's registry (and therefore
        // its default Sync Set) does not know it, so it must not receive it.
        let second = fixture
            .a
            .vaults
            .create_vault(
                "Work".to_string(),
                "#112233".to_string(),
                MASTER_PW,
                &fixture.a.secret_key,
            )
            .unwrap();
        fixture.a.user.add_vault_to_user(second.id).unwrap();

        // B has made its scope explicit and withheld the new Vault. Its
        // default would otherwise follow the merged registry.
        fixture
            .b
            .sync
            .set_sync_set(&fixture.b.user_id, [fixture.vault_id])
            .unwrap();

        fixture.sync();

        assert!(
            !fixture.b.vaults.has_vault_file(second.id),
            "a Vault outside B's Sync Set must not be sent or requested"
        );
        // The registry is shared state and learns that the Vault exists; the
        // Sync Set is what keeps its file off this Device.
        assert!(fixture
            .b
            .user
            .get_shared_profile(&fixture.b.user_id)
            .unwrap()
            .vaults
            .contains(&second.id));
        assert!(!fixture
            .b
            .sync
            .sync_set(
                &fixture.b.user_id,
                &fixture
                    .b
                    .user
                    .get_shared_profile(&fixture.b.user_id)
                    .unwrap()
                    .vaults,
            )
            .contains(&second.id));
    }

    #[test]
    fn test_an_unregistered_vault_file_is_ignored_until_explicitly_adopted() {
        let mut fixture = Fixture::new();

        let second = fixture
            .a
            .vaults
            .create_vault(
                "Copied".to_string(),
                "#112233".to_string(),
                MASTER_PW,
                &fixture.a.secret_key,
            )
            .unwrap();
        // The file exists on disk but is deliberately absent from the shared
        // Vault Registry, as if it had been copied in by hand.
        fixture.a.user.remove_vault_from_user(second.id).unwrap();

        let registry = fixture
            .a
            .user
            .get_shared_profile(&fixture.a.user_id)
            .unwrap()
            .vaults;
        assert_eq!(
            fixture
                .a
                .vaults
                .unregistered_vault_files(&registry)
                .unwrap(),
            vec![second.id],
            "an unregistered file is surfaced for adoption, never auto-adopted"
        );

        fixture.sync();
        assert!(
            !fixture.b.vaults.has_vault_file(second.id),
            "sync must ignore a file absent from the Vault Registry"
        );

        // Explicit adoption shares it: registry *and* Sync Set.
        fixture.a.user.add_vault_to_user(second.id).unwrap();
        fixture
            .a
            .sync
            .adopt(&fixture.a.user_id, second.id, &registry)
            .unwrap();
        fixture.sync();

        assert!(
            fixture.b.vaults.has_vault_file(second.id),
            "after adoption the Vault is sent and installed"
        );
        assert!(fixture
            .b
            .user
            .get_shared_profile(&fixture.b.user_id)
            .unwrap()
            .vaults
            .contains(&second.id));
    }

    #[test]
    fn test_dot_old_migration_backups_never_enter_a_transfer() {
        let mut fixture = Fixture::new();

        let backup = fixture
            .a
            .dir
            .join("vaults")
            .join(format!("{}.vault.OLD", fixture.vault_id));
        std::fs::write(&backup, b"not a live vault").unwrap();

        let registry = fixture
            .a
            .user
            .get_shared_profile(&fixture.a.user_id)
            .unwrap()
            .vaults;
        assert!(
            !fixture
                .a
                .vaults
                .unregistered_vault_files(&registry)
                .unwrap()
                .contains(&fixture.vault_id),
            ".OLD is not a candidate for adoption"
        );

        fixture.sync();
        assert!(
            !fixture
                .b
                .dir
                .join("vaults")
                .join(format!("{}.vault.OLD", fixture.vault_id))
                .exists(),
            "a migration backup must never be transferred"
        );
    }

    #[test]
    fn test_conflicts_pause_with_clean_merges_saved_and_never_persist_a_preview() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // Contested: display_name and the secret password. Clean: A changes
        // username, B changes email.
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("A-name".to_string()),
                None,
                None,
                None,
                None,
                Some(NewItemData::Account(AccountData {
                    username: "alice-a".to_string(),
                    email: None,
                })),
            )
            .unwrap();
        fixture
            .a
            .vaults
            .update_item_secret(
                fixture.vault_id,
                fixture.item_id,
                "password",
                Some("pw-a".to_string()),
            )
            .unwrap();

        fixture
            .b
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("B-name".to_string()),
                None,
                None,
                None,
                None,
                Some(NewItemData::Account(AccountData {
                    username: "alice".to_string(),
                    email: Some("b@example.com".to_string()),
                })),
            )
            .unwrap();
        fixture
            .b
            .vaults
            .update_item_secret(
                fixture.vault_id,
                fixture.item_id,
                "password",
                Some("pw-b".to_string()),
            )
            .unwrap();

        let (a_outcome, b_outcome) = fixture.sync();

        assert_eq!(a_outcome.pending.len(), 2);
        assert_eq!(b_outcome.pending.len(), 2);
        assert!(
            a_outcome
                .report
                .conflicts
                .iter()
                .any(|conflict| conflict.conflict.is_secret
                    && conflict.conflict.field_key == "password"
                    && conflict.vault_id == fixture.vault_id),
            "a secret conflict must carry its Vault id"
        );

        // The deterministic preview must not be written on either side:
        // each Device still holds its own contested values.
        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("A-name")
        );
        assert_eq!(
            live_item(&fixture.b, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("B-name")
        );
        assert_eq!(
            fixture
                .a
                .vaults
                .get_item_secret(fixture.vault_id, fixture.item_id, "password")
                .unwrap()
                .as_deref(),
            Some("pw-a")
        );
        assert_eq!(
            fixture
                .b
                .vaults
                .get_item_secret(fixture.vault_id, fixture.item_id, "password")
                .unwrap()
                .as_deref(),
            Some("pw-b")
        );

        // Clean field merges were still saved in the same sync.
        let a_item = live_item(&fixture.a, fixture.vault_id, fixture.item_id);
        let b_item = live_item(&fixture.b, fixture.vault_id, fixture.item_id);
        assert_eq!(account_data(a_item).username, "alice-a");
        assert_eq!(account_data(b_item).username, "alice-a");
        assert_eq!(account_data(a_item).email.as_deref(), Some("b@example.com"));
        assert_eq!(account_data(b_item).email.as_deref(), Some("b@example.com"));

        // The Safe View report never contains either secret candidate.
        let wire = serde_json::to_string(&a_outcome.report).unwrap();
        assert!(!wire.contains("pw-a") && !wire.contains("pw-b"), "{wire}");

        // Resolve display_name on A by choosing the remote value; B chooses
        // the remote (A's) password. Only now may the values be written.
        let display = pending_for(&a_outcome, "display_name");
        assert!(fixture
            .a
            .vaults
            .resolve_conflict_field(
                fixture.vault_id,
                &display.scope,
                &display.field_key,
                display.is_secret,
                display.local_version,
                display.remote.as_ref(),
            )
            .unwrap());
        let password = pending_for(&b_outcome, "password");
        assert!(fixture
            .b
            .vaults
            .resolve_conflict_field(
                fixture.vault_id,
                &password.scope,
                &password.field_key,
                password.is_secret,
                password.local_version,
                password.remote.as_ref(),
            )
            .unwrap());

        fixture.sync();

        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("B-name")
        );
        assert_eq!(
            live_item(&fixture.b, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("B-name")
        );
        assert_eq!(
            fixture
                .a
                .vaults
                .get_item_secret(fixture.vault_id, fixture.item_id, "password")
                .unwrap()
                .as_deref(),
            Some("pw-a")
        );
        assert_eq!(
            fixture
                .b
                .vaults
                .get_item_secret(fixture.vault_id, fixture.item_id, "password")
                .unwrap()
                .as_deref(),
            Some("pw-a")
        );
    }

    #[test]
    fn test_a_vault_metadata_conflict_carries_the_vault_id_and_keeps_local_metadata() {
        let mut fixture = Fixture::new();
        fixture.sync();

        fixture
            .a
            .vaults
            .update_vault(fixture.vault_id, Some("A-Vault".to_string()), None)
            .unwrap();
        fixture
            .b
            .vaults
            .update_vault(fixture.vault_id, Some("B-Vault".to_string()), None)
            .unwrap();

        let (a_outcome, _) = fixture.sync();
        let conflict = a_outcome
            .report
            .conflicts
            .iter()
            .find(|conflict| conflict.conflict.field_key == "name")
            .expect("a vault metadata conflict is surfaced");
        assert_eq!(conflict.vault_id, fixture.vault_id);
        assert!(conflict.conflict.is_vault_metadata());
        assert_eq!(
            fixture.a.vaults.get_vault(fixture.vault_id).unwrap().name,
            "A-Vault",
            "the preview must not overwrite the local name"
        );
        assert_eq!(
            fixture.b.vaults.get_vault(fixture.vault_id).unwrap().name,
            "B-Vault"
        );

        let pending = pending_for(&a_outcome, "name");
        fixture
            .a
            .vaults
            .resolve_conflict_field(
                fixture.vault_id,
                &pending.scope,
                &pending.field_key,
                pending.is_secret,
                pending.local_version,
                pending.remote.as_ref(),
            )
            .unwrap();
        fixture.sync();

        assert_eq!(
            fixture.a.vaults.get_vault(fixture.vault_id).unwrap().name,
            "B-Vault"
        );
        assert_eq!(
            fixture.b.vaults.get_vault(fixture.vault_id).unwrap().name,
            "B-Vault"
        );
    }

    #[test]
    fn test_sync_refuses_while_logged_out_and_for_a_revoked_peer() {
        let mut fixture = Fixture::new();
        let peer = fixture
            .a
            .net
            .discover()
            .unwrap()
            .into_iter()
            .find(|peer| peer.device_id() == fixture.b.user.device_id())
            .unwrap();

        fixture.a.user.logout().unwrap();
        let mut noop = |_: SyncProgress| {};
        let scope = fixture.a.scope();
        let credentials = SyncCredentials {
            master_password: MASTER_PW,
            secret_key: &fixture.a.secret_key,
        };
        let mut engine = SyncEngine::new(
            &fixture.a.net,
            &mut fixture.a.vaults,
            &fixture.a.user,
            scope,
            credentials,
        );
        assert!(matches!(
            engine.start_with_peer(&peer, &mut noop),
            Err(VaultError::SyncNotLoggedIn)
        ));

        // Sign back in, revoke B, and the distinct revocation error wins
        // before anything is sent.
        let _ = fixture.a.user.login(&fixture.a.user_id, MASTER_PW).unwrap();
        fixture
            .a
            .user
            .revoke_device(fixture.b.user.device_id(), Utc::now())
            .unwrap();
        let scope = fixture.a.scope();
        let credentials = SyncCredentials {
            master_password: MASTER_PW,
            secret_key: &fixture.a.secret_key,
        };
        let mut engine = SyncEngine::new(
            &fixture.a.net,
            &mut fixture.a.vaults,
            &fixture.a.user,
            scope,
            credentials,
        );
        assert!(matches!(
            engine.start_with_peer(&peer, &mut noop),
            Err(VaultError::SyncPeerRevoked(_))
        ));
    }

    #[test]
    fn test_network_failures_map_to_distinct_sync_errors() {
        assert!(matches!(
            SyncEngine::map_connect_error(NetworkError::ProtocolMismatch {
                local: 1,
                remote: 2
            }),
            VaultError::SyncProtocolMismatch {
                local: 1,
                remote: 2
            }
        ));
        assert!(matches!(
            SyncEngine::map_connect_error(NetworkError::SchemaMismatch {
                local: 3,
                remote: 4
            }),
            VaultError::SyncSchemaMismatch {
                local: 3,
                remote: 4
            }
        ));
        assert!(matches!(
            SyncEngine::map_connect_error(NetworkError::AuthenticationFailed),
            VaultError::SyncAuthenticationFailed
        ));
        assert!(matches!(
            SyncEngine::map_connect_error(NetworkError::DiscoveryFailed("boom".into())),
            VaultError::SyncDiscoveryFailed(_)
        ));
        assert!(matches!(
            SyncEngine::map_channel_error(NetworkError::ConnectionClosed),
            VaultError::SyncInterrupted(_)
        ));
    }

    // ============ Interrupted transfer ============

    struct DroppingNetwork<'a> {
        inner: &'a InMemoryNetwork,
        max_received: usize,
    }

    impl PeerNetwork for DroppingNetwork<'_> {
        fn start_advertising(&self) -> Result<(), NetworkError> {
            self.inner.start_advertising()
        }

        fn stop_advertising(&self) -> Result<(), NetworkError> {
            self.inner.stop_advertising()
        }

        fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
            self.inner.discover()
        }

        fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
            Ok(Box::new(DroppingChannel {
                inner: self.inner.connect(peer)?,
                remaining_receives: self.max_received,
            }))
        }

        fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
            Ok(self.inner.accept()?.map(|inner| {
                Box::new(DroppingChannel {
                    inner,
                    remaining_receives: self.max_received,
                }) as Box<dyn PeerChannel>
            }))
        }
    }

    struct DroppingChannel {
        inner: Box<dyn PeerChannel>,
        remaining_receives: usize,
    }

    impl PeerChannel for DroppingChannel {
        fn peer_device_id(&self) -> DeviceId {
            self.inner.peer_device_id()
        }

        fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
            self.inner.send(message)
        }

        fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
            if self.remaining_receives == 0 {
                return Err(NetworkError::ConnectionClosed);
            }
            self.remaining_receives -= 1;
            self.inner.receive()
        }

        fn receive_timeout(
            &mut self,
            _timeout: std::time::Duration,
        ) -> Result<SyncMessage, NetworkError> {
            // The simulated drop is immediate; it is not a deadline expiry.
            self.receive()
        }

        fn close(&mut self) {
            self.inner.close()
        }
    }

    #[test]
    fn test_an_interrupted_sync_leaves_the_previous_vault_readable_and_is_retryable() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // B adds an item so this sync has real content to transfer.
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("Renamed on A".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let dropping = DroppingNetwork {
            inner: &fixture.b.net,
            // B accepts the scope (1) and one vault frame (2), then the
            // channel drops before Complete.
            max_received: 2,
        };

        let peer = fixture
            .a
            .net
            .discover()
            .unwrap()
            .into_iter()
            .find(|peer| peer.device_id() == fixture.b.user.device_id())
            .unwrap();
        let mut noop = |_: SyncProgress| {};

        let a_scope = fixture.a.scope();
        let a_credentials = SyncCredentials {
            master_password: MASTER_PW,
            secret_key: &fixture.a.secret_key,
        };
        let mut engine_a = SyncEngine::new(
            &fixture.a.net,
            &mut fixture.a.vaults,
            &fixture.a.user,
            a_scope,
            a_credentials,
        );
        let start = engine_a.start_with_peer(&peer, &mut noop).unwrap();

        let b_scope = fixture.b.scope();
        let b_credentials = SyncCredentials {
            master_password: MASTER_PW,
            secret_key: &fixture.b.secret_key,
        };
        let mut engine_b = SyncEngine::new(
            &dropping,
            &mut fixture.b.vaults,
            &fixture.b.user,
            b_scope,
            b_credentials,
        );
        let accepted = engine_b.accept(&mut noop).unwrap().unwrap();

        let sent = engine_a.send_offers(start, &mut noop).unwrap();
        assert!(matches!(
            engine_b.receive_offers(accepted, &mut noop),
            Err(VaultError::SyncInterrupted(_))
        ));
        assert!(matches!(
            engine_a.collect(sent, &mut noop),
            Err(VaultError::SyncInterrupted(_))
        ));

        // Whatever was merged before the drop is saved atomically; the vault
        // file still unlocks if this Device restarts.
        fixture.b.vaults.lock_vault(Some(fixture.vault_id));
        let readable = fixture
            .b
            .vaults
            .unlock_vault(fixture.vault_id, MASTER_PW, &fixture.b.secret_key)
            .expect("an interrupted sync must leave the previous vault readable");
        assert!(readable.items.iter().any(|item| item.id == fixture.item_id));

        // A retry completes the same transfer.
        fixture.sync();
        assert_eq!(
            fixture.a.vaults.get_vault(fixture.vault_id).unwrap(),
            fixture.b.vaults.get_vault(fixture.vault_id).unwrap()
        );
        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("Renamed on A")
        );
    }

    // ============ Pairing gates the sync seam ============

    /// A [`PeerNetwork`] whose channels claim to belong to another Device.
    struct ImpersonatingNetwork<'a> {
        inner: &'a InMemoryNetwork,
        claims: DeviceId,
    }

    impl PeerNetwork for ImpersonatingNetwork<'_> {
        fn start_advertising(&self) -> Result<(), NetworkError> {
            self.inner.start_advertising()
        }

        fn stop_advertising(&self) -> Result<(), NetworkError> {
            self.inner.stop_advertising()
        }

        fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
            self.inner.discover()
        }

        fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
            Ok(Box::new(ImpersonatingChannel {
                inner: self.inner.connect(peer)?,
                claims: self.claims,
            }))
        }

        fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
            self.inner.accept()
        }
    }

    struct ImpersonatingChannel {
        inner: Box<dyn PeerChannel>,
        claims: DeviceId,
    }

    impl PeerChannel for ImpersonatingChannel {
        fn peer_device_id(&self) -> DeviceId {
            self.claims
        }

        fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
            self.inner.send(message)
        }

        fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
            self.inner.receive()
        }

        fn receive_timeout(
            &mut self,
            timeout: std::time::Duration,
        ) -> Result<SyncMessage, NetworkError> {
            self.inner.receive_timeout(timeout)
        }

        fn close(&mut self) {
            self.inner.close()
        }
    }

    /// A [`PeerNetwork`] whose channels stop answering after `frames` replies,
    /// standing in for a transport-level read timeout.
    struct SilentAfterNetwork<'a> {
        inner: &'a InMemoryNetwork,
        frames: usize,
    }

    impl PeerNetwork for SilentAfterNetwork<'_> {
        fn start_advertising(&self) -> Result<(), NetworkError> {
            self.inner.start_advertising()
        }

        fn stop_advertising(&self) -> Result<(), NetworkError> {
            self.inner.stop_advertising()
        }

        fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
            self.inner.discover()
        }

        fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
            Ok(Box::new(SilentChannel {
                inner: self.inner.connect(peer)?,
                remaining: self.frames,
            }))
        }

        fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
            Ok(self.inner.accept()?.map(|inner| {
                Box::new(SilentChannel {
                    inner,
                    remaining: self.frames,
                }) as Box<dyn PeerChannel>
            }))
        }
    }

    struct SilentChannel {
        inner: Box<dyn PeerChannel>,
        remaining: usize,
    }

    impl SilentChannel {
        fn tick(&mut self) -> Result<(), NetworkError> {
            if self.remaining == 0 {
                return Err(NetworkError::Timeout);
            }
            self.remaining -= 1;
            Ok(())
        }
    }

    impl PeerChannel for SilentChannel {
        fn peer_device_id(&self) -> DeviceId {
            self.inner.peer_device_id()
        }

        fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
            self.inner.send(message)
        }

        fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
            self.tick()?;
            self.inner.receive()
        }

        fn receive_timeout(
            &mut self,
            timeout: std::time::Duration,
        ) -> Result<SyncMessage, NetworkError> {
            self.tick()?;
            self.inner.receive_timeout(timeout)
        }

        fn close(&mut self) {
            self.inner.close()
        }
    }

    /// A [`PeerNetwork`] whose channel waits in `receive_timeout` until the
    /// test releases it, standing in for a transport blocked on the network.
    struct GatedNetwork<'a> {
        inner: &'a InMemoryNetwork,
        gate: Arc<Gate>,
    }

    impl PeerNetwork for GatedNetwork<'_> {
        fn start_advertising(&self) -> Result<(), NetworkError> {
            self.inner.start_advertising()
        }

        fn stop_advertising(&self) -> Result<(), NetworkError> {
            self.inner.stop_advertising()
        }

        fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
            self.inner.discover()
        }

        fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
            Ok(Box::new(GatedChannel {
                inner: self.inner.connect(peer)?,
                gate: Arc::clone(&self.gate),
            }))
        }

        fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
            self.inner.accept()
        }
    }

    /// Handshake between the engine thread and the test thread.
    struct Gate {
        entered: Mutex<bool>,
        released: Mutex<bool>,
        signal: std::sync::Condvar,
    }

    impl Gate {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                entered: Mutex::new(false),
                released: Mutex::new(false),
                signal: std::sync::Condvar::new(),
            })
        }

        /// Block until the engine is waiting for a frame.
        fn wait_until_waiting(&self) {
            let mut entered = self.entered.lock().unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !*entered {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                assert!(!remaining.is_zero(), "the engine never waited for a frame");
                let (guard, _) = self.signal.wait_timeout(entered, remaining).unwrap();
                entered = guard;
            }
        }

        fn release(&self) {
            *self.released.lock().unwrap() = true;
            self.signal.notify_all();
        }
    }

    struct GatedChannel {
        inner: Box<dyn PeerChannel>,
        gate: Arc<Gate>,
    }

    impl PeerChannel for GatedChannel {
        fn peer_device_id(&self) -> DeviceId {
            self.inner.peer_device_id()
        }

        fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
            self.inner.send(message)
        }

        fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
            self.inner.receive()
        }

        fn receive_timeout(&mut self, timeout: Duration) -> Result<SyncMessage, NetworkError> {
            *self.gate.entered.lock().unwrap() = true;
            self.gate.signal.notify_all();

            let mut released = self.gate.released.lock().unwrap();
            let deadline = std::time::Instant::now() + timeout;
            while !*released {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                let (guard, _) = self.gate.signal.wait_timeout(released, remaining).unwrap();
                released = guard;
            }

            Err(NetworkError::Timeout)
        }

        fn close(&mut self) {
            self.inner.close();
        }
    }

    macro_rules! device_engine {
        ($fixture:ident, $side:ident) => {
            SyncEngine::new(
                &$fixture.$side.net,
                &mut $fixture.$side.vaults,
                &$fixture.$side.user,
                $fixture.$side.sync.scope(&$fixture.$side.user_id),
                SyncCredentials {
                    master_password: MASTER_PW,
                    secret_key: &$fixture.$side.secret_key,
                },
            )
        };
    }

    fn shared_vaults(device: &Device) -> VaultRegistry {
        device
            .user
            .get_shared_profile(&device.user_id)
            .unwrap()
            .vaults
    }

    fn discovered_b(fixture: &Fixture) -> DiscoveredPeer {
        fixture
            .a
            .net
            .discover()
            .unwrap()
            .into_iter()
            .find(|peer| peer.device_id() == fixture.b.user.device_id())
            .expect("b is advertising")
    }

    #[test]
    fn test_an_unpaired_advertiser_is_refused_before_any_offer() {
        let mut fixture = Fixture::new();

        // The transports know each other (the in-memory seam models mutual
        // pairing), but this User's registry does not: no pairing ever
        // happened. An unknown Device is not "merely unrevoked".
        let mut shared = fixture
            .a
            .user
            .get_shared_profile(&fixture.a.user_id)
            .unwrap();
        shared.devices = crate::DeviceRegistry::new();
        fixture.a.user.save_shared_profile(&shared).unwrap();

        let peer = discovered_b(&fixture);
        let mut noop = |_: SyncProgress| {};
        let mut engine = device_engine!(fixture, a);

        assert!(matches!(
            engine.start_with_peer(&peer, &mut noop),
            Err(VaultError::SyncPeerNotPaired(_))
        ));
        assert!(
            fixture.b.net.accept().unwrap().is_none(),
            "refusal must happen before a channel is opened, so nothing is offered"
        );
    }

    #[test]
    fn test_a_channel_that_leads_to_another_device_is_refused() {
        let mut fixture = Fixture::new();
        let peer = discovered_b(&fixture);
        let impostor = Uuid::new_v4();
        let impersonating = ImpersonatingNetwork {
            inner: &fixture.a.net,
            claims: impostor,
        };

        let mut noop = |_: SyncProgress| {};
        let mut engine = SyncEngine::new(
            &impersonating,
            &mut fixture.a.vaults,
            &fixture.a.user,
            fixture.a.sync.scope(&fixture.a.user_id),
            SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &fixture.a.secret_key,
            },
        );

        assert!(matches!(
            engine.start_with_peer(&peer, &mut noop),
            Err(VaultError::SyncWrongPeer)
        ));
    }

    // ============ Timeouts, cancellation and one sync at a time ============

    #[test]
    fn test_a_silent_peer_times_out_instead_of_waiting_forever() {
        let mut fixture = Fixture::new();
        let peer = discovered_b(&fixture);
        let silent = SilentAfterNetwork {
            inner: &fixture.a.net,
            frames: 0,
        };

        let mut noop = |_: SyncProgress| {};
        let mut engine = SyncEngine::new(
            &silent,
            &mut fixture.a.vaults,
            &fixture.a.user,
            fixture.a.sync.scope(&fixture.a.user_id),
            SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &fixture.a.secret_key,
            },
        );
        let start = engine.start_with_peer(&peer, &mut noop).unwrap();

        assert!(matches!(
            engine.send_offers(start, &mut noop),
            Err(VaultError::SyncTimeout(_))
        ));
    }

    #[test]
    fn test_a_cancelled_sync_sends_nothing() {
        let mut fixture = Fixture::new();
        let peer = discovered_b(&fixture);
        let cancel = SyncCancel::new();
        cancel.cancel();

        let mut noop = |_: SyncProgress| {};
        let mut engine = device_engine!(fixture, a).with_cancel(cancel);

        assert!(matches!(
            engine.start_with_peer(&peer, &mut noop),
            Err(VaultError::SyncCancelled)
        ));
        assert!(fixture.b.net.accept().unwrap().is_none());
    }

    #[test]
    fn test_cancel_reaches_a_sync_already_waiting_for_a_frame() {
        let mut fixture = Fixture::new();
        let peer = discovered_b(&fixture);
        let gate = Gate::new();
        let gated = GatedNetwork {
            inner: &fixture.a.net,
            gate: Arc::clone(&gate),
        };
        let control = Arc::new(SyncControl::default());

        let vaults = &mut fixture.a.vaults;
        let user = &fixture.a.user;
        let secret_key = &fixture.a.secret_key;
        let user_id = fixture.a.user_id;
        let sync = &fixture.a.sync;

        let outcome = std::thread::scope(|scope| {
            let runner = Arc::clone(&control);
            let handle = scope.spawn(move || {
                let lease = runner.begin().expect("the first sync claims the slot");
                let mut noop = |_: SyncProgress| {};
                let mut engine = SyncEngine::new(
                    &gated,
                    vaults,
                    user,
                    sync.scope(&user_id),
                    SyncCredentials {
                        master_password: MASTER_PW,
                        secret_key,
                    },
                )
                .with_cancel(lease.token());

                let start = engine.start_with_peer(&peer, &mut noop).unwrap();
                engine.send_offers(start, &mut noop)
            });

            // Cancel only after the exchange is genuinely blocked on the
            // transport, so this cannot pass by cancelling before it starts.
            gate.wait_until_waiting();
            control.cancel();
            gate.release();

            handle.join().unwrap()
        });

        assert!(
            matches!(outcome, Err(VaultError::SyncCancelled)),
            "cancel must interrupt a sync waiting on the transport"
        );
        assert!(!control.is_active(), "the cancelled sync releases the slot");
    }

    #[test]
    fn test_two_simultaneous_syncs_do_not_interleave() {
        let control = SyncControl::default();

        let first = control.begin().unwrap();
        assert!(control.is_active());
        assert!(matches!(
            control.begin(),
            Err(VaultError::SyncRefused(_))
        ));

        // Cancelling reaches the running sync without any lock the exchange
        // is holding, which is why the token lives in this control rather
        // than in the application state.
        assert!(!first.token().is_cancelled());
        control.cancel();
        assert!(first.token().is_cancelled());

        // Finishing releases the slot even after a failure.
        drop(first);
        assert!(!control.is_active());
        assert!(control.begin().is_ok());
    }

    // ============ Integration-boundary checks ============

    #[test]
    fn test_a_vault_blob_from_another_schema_is_refused_at_the_boundary() {
        let mut fixture = Fixture::new();
        fixture.sync();

        let mut tampered = fixture.b.vaults.get_vault(fixture.vault_id).unwrap().clone();
        tampered.schema_version = CURRENT_SCHEMA_VERSION + 1;
        fixture.b.vaults.replace_vault(tampered).unwrap();

        let (a_result, _) = fixture.sync_attempt();

        assert!(matches!(
            a_result,
            Err(VaultError::SyncSchemaMismatch { local, remote })
                if local == CURRENT_SCHEMA_VERSION && remote == CURRENT_SCHEMA_VERSION + 1
        ));
        assert_eq!(
            fixture
                .a
                .vaults
                .get_vault(fixture.vault_id)
                .unwrap()
                .schema_version,
            CURRENT_SCHEMA_VERSION,
            "the refused blob must not be integrated"
        );
    }

    #[test]
    fn test_an_item_kind_disagreement_is_refused_rather_than_holding_the_session_open() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // No local operation can change an Item's kind, so this is a
        // malformed peer copy: there is no honest value to offer the User.
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("Renamed on A".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let mut tampered = fixture.b.vaults.get_vault(fixture.vault_id).unwrap().clone();
        let device = fixture.b.vaults.device_id();
        let item = tampered
            .items
            .iter_mut()
            .find(|item| item.id == fixture.item_id)
            .unwrap();
        item.kind = Some(crate::ItemKind::Note(crate::NoteData {
            title: Some("Not an account".to_string()),
            body: "tampered".to_string(),
        }));
        item.clock.increment(device);
        fixture.b.vaults.replace_vault(tampered).unwrap();

        let (_a_result, b_result) = fixture.sync_attempt();

        assert!(
            matches!(b_result, Err(VaultError::SyncRefused(_))),
            "the side that merged the malformed copy refuses it"
        );
        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("Renamed on A"),
            "nothing is written for a Vault whose merge was refused"
        );
        assert!(
            account_data(live_item(&fixture.a, fixture.vault_id, fixture.item_id))
                .username
                .eq("alice")
        );
    }

    #[test]
    fn test_resolving_a_conflict_after_an_intervening_local_edit_is_refused() {
        let mut fixture = Fixture::new();
        fixture.sync();

        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("A-name".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        fixture
            .b
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("B-name".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let (a_outcome, _) = fixture.sync();
        let display = pending_for(&a_outcome, "display_name");
        let captured_version = display.local_version;
        let candidate = display.remote.clone();

        // The User edits the contested field before answering the prompt.
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("A-newer".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let result = fixture.a.vaults.resolve_conflict_field(
            fixture.vault_id,
            &display.scope,
            &display.field_key,
            display.is_secret,
            captured_version,
            candidate.as_ref(),
        );

        assert!(matches!(
            result,
            Err(VaultError::SyncConflictResolution(_))
        ));
        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("A-newer"),
            "the newer local edit must survive a stale choice"
        );
    }

    // ============ Vault deletion and the three removal operations ============

    #[test]
    fn test_deleting_a_vault_travels_as_a_tombstone_and_becomes_local_trash() {
        let mut fixture = Fixture::new();
        fixture.sync();

        fixture
            .a
            .user
            .delete_vault(fixture.vault_id, Utc::now())
            .unwrap();
        fixture.a.vaults.move_to_trash(fixture.vault_id).unwrap();

        fixture.sync();

        for device in [&fixture.a, &fixture.b] {
            let registry = shared_vaults(device);
            assert!(registry.is_tombstoned(&fixture.vault_id));
            assert!(!registry.contains(&fixture.vault_id));
            assert!(
                !device.vaults.has_vault_file(fixture.vault_id),
                "a tombstoned Vault has no live file on any Device"
            );
            assert!(
                device.vaults.vault_trash_path(fixture.vault_id).exists(),
                "a deleted Vault is kept as trash, not destroyed"
            );
        }

        // Repeating the sync cannot resurrect it from a stale copy.
        fixture.sync();
        assert!(!fixture.b.vaults.has_vault_file(fixture.vault_id));
        assert!(shared_vaults(&fixture.b).is_tombstoned(&fixture.vault_id));
    }

    #[test]
    fn test_stop_syncing_remove_local_copy_and_delete_everywhere_stay_distinct() {
        let mut fixture = Fixture::new();
        fixture.sync();
        let user_id = fixture.b.user_id;

        // 1. Stop syncing: the copy stays, keeps working, drifts, and the
        //    shared registry is untouched.
        let registry = shared_vaults(&fixture.b);
        fixture
            .b
            .sync
            .set_vault_syncing(&user_id, fixture.vault_id, false, &registry)
            .unwrap();
        fixture
            .b
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("B-only".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        fixture.sync();

        assert!(fixture.b.vaults.has_vault_file(fixture.vault_id));
        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("Mail"),
            "a Vault outside the Sync Set is not sent"
        );
        let registry = shared_vaults(&fixture.b);
        assert!(registry.contains(&fixture.vault_id));
        assert!(!registry.is_tombstoned(&fixture.vault_id));

        // 2. Remove local copy: this Device's file only. No Tombstone, and
        //    the other Device still has its copy. It leaves the Sync Set, or
        //    the next sync would immediately install it again.
        fixture.b.vaults.remove_local_copy(fixture.vault_id).unwrap();
        fixture
            .b
            .sync
            .set_vault_syncing(&user_id, fixture.vault_id, false, &shared_vaults(&fixture.b))
            .unwrap();

        fixture.sync();

        assert!(!fixture.b.vaults.has_vault_file(fixture.vault_id));
        assert!(
            !fixture.b.vaults.vault_trash_path(fixture.vault_id).exists(),
            "removing a local copy is not a deletion"
        );
        assert!(
            fixture.a.vaults.has_vault_file(fixture.vault_id),
            "other Devices keep their copy"
        );
        let registry = shared_vaults(&fixture.b);
        assert!(registry.contains(&fixture.vault_id));
        assert!(!registry.is_tombstoned(&fixture.vault_id));

        // Re-enabling brings it back, from the peer.
        fixture
            .b
            .sync
            .set_vault_syncing(&user_id, fixture.vault_id, true, &registry)
            .unwrap();
        fixture.sync();
        assert!(fixture.b.vaults.has_vault_file(fixture.vault_id));

        // 3. Delete everywhere: a Tombstone that travels, and trash on both.
        fixture
            .a
            .user
            .delete_vault(fixture.vault_id, Utc::now())
            .unwrap();
        fixture.a.vaults.move_to_trash(fixture.vault_id).unwrap();

        fixture.sync();

        assert!(shared_vaults(&fixture.a).is_tombstoned(&fixture.vault_id));
        assert!(shared_vaults(&fixture.b).is_tombstoned(&fixture.vault_id));
        assert!(!fixture.b.vaults.has_vault_file(fixture.vault_id));
        assert!(fixture.b.vaults.vault_trash_path(fixture.vault_id).exists());
    }

    #[test]
    fn test_re_enabling_a_vault_merges_the_divergence_it_accrued() {
        let mut fixture = Fixture::new();
        fixture.sync();
        let user_id = fixture.b.user_id;

        fixture
            .b
            .sync
            .set_vault_syncing(&user_id, fixture.vault_id, false, &shared_vaults(&fixture.b))
            .unwrap();

        // Both sides edit while B is out of the Sync Set.
        let note = fixture
            .b
            .vaults
            .add_item(
                fixture.vault_id,
                Some("B-only note".to_string()),
                None,
                NewItemData::Note(crate::NoteData {
                    title: None,
                    body: "written while not syncing".to_string(),
                }),
                None,
            )
            .unwrap();
        let note_id = match note {
            crate::SafeItem::Note(note) => note.id,
            _ => panic!("expected a note"),
        };
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("Renamed while B was away".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        fixture.sync();
        assert!(
            !fixture
                .a
                .vaults
                .get_vault(fixture.vault_id)
                .unwrap()
                .items
                .iter()
                .any(|item| item.id == note_id),
            "the divergence stays local until sync is re-enabled"
        );

        fixture
            .b
            .sync
            .set_vault_syncing(&user_id, fixture.vault_id, true, &shared_vaults(&fixture.b))
            .unwrap();
        fixture.sync();

        for device in [&fixture.a, &fixture.b] {
            let vault = device.vaults.get_vault(fixture.vault_id).unwrap();
            assert!(vault.items.iter().any(|item| item.id == note_id));
            assert_eq!(
                live_item(device, fixture.vault_id, fixture.item_id)
                    .display_name
                    .as_deref(),
                Some("Renamed while B was away")
            );
        }
    }

    #[test]
    fn test_re_enabling_a_vault_applies_a_tombstone_that_arrived_while_it_was_out() {
        let mut fixture = Fixture::new();
        fixture.sync();
        let user_id = fixture.b.user_id;

        fixture
            .b
            .sync
            .set_vault_syncing(&user_id, fixture.vault_id, false, &shared_vaults(&fixture.b))
            .unwrap();

        // The other Device deletes the Vault while B is not syncing it.
        fixture
            .a
            .user
            .delete_vault(fixture.vault_id, Utc::now())
            .unwrap();
        fixture.a.vaults.move_to_trash(fixture.vault_id).unwrap();
        fixture.sync();

        // B re-enables. The pending Tombstone has already arrived with the
        // registry, so there is nothing left to sync: the file is trash.
        fixture
            .b
            .sync
            .set_vault_syncing(&user_id, fixture.vault_id, true, &shared_vaults(&fixture.b))
            .unwrap();
        fixture.sync();

        assert!(shared_vaults(&fixture.b).is_tombstoned(&fixture.vault_id));
        assert!(!fixture.b.vaults.has_vault_file(fixture.vault_id));
        assert!(fixture.b.vaults.vault_trash_path(fixture.vault_id).exists());
        assert!(!fixture
            .b
            .sync
            .sync_set(&user_id, &shared_vaults(&fixture.b))
            .contains(&fixture.vault_id));
    }

    #[test]
    fn test_a_vault_created_on_the_peer_transfers_in_the_same_sync() {
        let mut fixture = Fixture::new();
        fixture.sync();

        let second = fixture
            .b
            .vaults
            .create_vault(
                "Created on B".to_string(),
                "#112233".to_string(),
                MASTER_PW,
                &fixture.b.secret_key,
            )
            .unwrap();
        fixture.b.user.add_vault_to_user(second.id).unwrap();

        fixture.sync();

        assert!(
            fixture.a.vaults.has_vault_file(second.id),
            "the initiator must learn the peer's new Vault in one round"
        );
        assert!(shared_vaults(&fixture.a).contains(&second.id));
    }

    // ============ Revocation and interruption during the transfer ============

    #[test]
    fn test_a_revocation_arriving_mid_sync_stops_the_transfer() {
        let mut fixture = Fixture::new();
        fixture.sync();
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("Renamed on A".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let peer = discovered_b(&fixture);
        let mut noop = |_: SyncProgress| {};

        let start = {
            let mut engine_a = device_engine!(fixture, a);
            engine_a.start_with_peer(&peer, &mut noop).unwrap()
        };
        let accepted = {
            let mut engine_b = device_engine!(fixture, b);
            engine_b.accept(&mut noop).unwrap().unwrap()
        };

        // B revokes A while the channel is open. A knows nothing about it.
        fixture
            .b
            .user
            .revoke_device(fixture.a.user.device_id(), Utc::now())
            .unwrap();

        {
            let mut engine_a = device_engine!(fixture, a);
            engine_a.send_offers(start, &mut noop).unwrap();
        }

        {
            let mut engine_b = device_engine!(fixture, b);
            assert!(matches!(
                engine_b.receive_offers(accepted, &mut noop),
                Err(VaultError::SyncPeerRevoked(_))
            ));
        }
        assert_eq!(
            fixture
                .b
                .vaults
                .get_vault(fixture.vault_id)
                .unwrap()
                .items
                .iter()
                .find(|item| item.id == fixture.item_id)
                .unwrap()
                .display_name
                .as_deref(),
            Some("Mail"),
            "a revoked peer's vault content must not be integrated"
        );
    }

    #[test]
    fn test_an_interruption_during_the_first_copy_install_never_leaves_half_a_vault() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // B no longer holds the Vault, so this sync is a first-copy install.
        fixture.b.vaults.remove_local_copy(fixture.vault_id).unwrap();
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("Renamed on A".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let mut noop = |_: SyncProgress| {};
        let peer = discovered_b(&fixture);

        // Drop after the scope and the vault frame: the install itself
        // completes before the channel fails.
        let dropping = DroppingNetwork {
            inner: &fixture.b.net,
            max_received: 2,
        };
        let start = {
            let mut engine_a = device_engine!(fixture, a);
            engine_a.start_with_peer(&peer, &mut noop).unwrap()
        };
        let accepted = {
            let mut engine_b = SyncEngine::new(
                &dropping,
                &mut fixture.b.vaults,
                &fixture.b.user,
                fixture.b.sync.scope(&fixture.b.user_id),
                SyncCredentials {
                    master_password: MASTER_PW,
                    secret_key: &fixture.b.secret_key,
                },
            );
            engine_b.accept(&mut noop).unwrap().unwrap()
        };
        {
            let mut engine_a = device_engine!(fixture, a);
            engine_a.send_offers(start, &mut noop).unwrap();
        }
        let mut engine_b = SyncEngine::new(
            &dropping,
            &mut fixture.b.vaults,
            &fixture.b.user,
            fixture.b.sync.scope(&fixture.b.user_id),
            SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &fixture.b.secret_key,
            },
        );
        assert!(matches!(
            engine_b.receive_offers(accepted, &mut noop),
            Err(VaultError::SyncInterrupted(_))
        ));
        drop(engine_b);
        fixture.b.vaults.lock_vault(Some(fixture.vault_id));
        assert!(
            fixture
                .b
                .vaults
                .unlock_vault(fixture.vault_id, MASTER_PW, &fixture.b.secret_key)
                .is_ok(),
            "the installed first copy must be whole and readable"
        );

        // Drop before the vault frame: no file is written at all.
        fixture.b.vaults.remove_local_copy(fixture.vault_id).unwrap();
        let dropping = DroppingNetwork {
            inner: &fixture.b.net,
            max_received: 1,
        };
        let start = {
            let mut engine_a = device_engine!(fixture, a);
            engine_a.start_with_peer(&peer, &mut noop).unwrap()
        };
        let accepted = {
            let mut engine_b = SyncEngine::new(
                &dropping,
                &mut fixture.b.vaults,
                &fixture.b.user,
                fixture.b.sync.scope(&fixture.b.user_id),
                SyncCredentials {
                    master_password: MASTER_PW,
                    secret_key: &fixture.b.secret_key,
                },
            );
            engine_b.accept(&mut noop).unwrap().unwrap()
        };
        {
            let mut engine_a = device_engine!(fixture, a);
            engine_a.send_offers(start, &mut noop).unwrap();
        }
        let mut engine_b = SyncEngine::new(
            &dropping,
            &mut fixture.b.vaults,
            &fixture.b.user,
            fixture.b.sync.scope(&fixture.b.user_id),
            SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &fixture.b.secret_key,
            },
        );
        assert!(matches!(
            engine_b.receive_offers(accepted, &mut noop),
            Err(VaultError::SyncInterrupted(_))
        ));
        drop(engine_b);
        assert!(
            !fixture.b.vaults.has_vault_file(fixture.vault_id),
            "nothing may be half-installed"
        );
        assert!(!fixture
            .b
            .vaults
            .vault_trash_path(fixture.vault_id)
            .exists());
    }

    // ============ End-to-end user scenarios (#45) ============
    //
    // These drive two real managers over two vault directories and the
    // in-memory network seam, and assert only on what a User can observe:
    // Vault content, secrets, Tombstones, registry state, errors, and files.

    #[test]
    fn test_pairing_alone_leaves_both_devices_with_the_same_device_and_vault_lists() {
        let fixture = Fixture::new();

        let a_shared = fixture
            .a
            .user
            .get_shared_profile(&fixture.a.user_id)
            .unwrap();
        let b_shared = fixture
            .b
            .user
            .get_shared_profile(&fixture.b.user_id)
            .unwrap();

        assert_eq!(
            a_shared.vault_ids(),
            b_shared.vault_ids(),
            "both Devices must show one list of Vaults"
        );
        assert_eq!(a_shared.vault_ids(), vec![fixture.vault_id]);
        assert_eq!(
            a_shared.devices, b_shared.devices,
            "pairing admits the joining Device on both sides"
        );
        assert!(a_shared.devices.contains(&fixture.a.user.device_id()));
        assert!(a_shared.devices.contains(&fixture.b.user.device_id()));

        // The joining Device holds the secret key transferred at pairing, so
        // its copy opens without the source Device's password ever moving.
        assert_eq!(
            fixture
                .b
                .vaults
                .get_item_secret(fixture.vault_id, fixture.item_id, "password")
                .unwrap()
                .as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn test_disjoint_field_edits_on_two_devices_merge_without_a_prompt() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // A renames the Item and tags it; B rotates the password and
        // favourites it. No field is touched on both Devices.
        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("Mail (work)".to_string()),
                None,
                None,
                Some(vec!["work".to_string()]),
                None,
                None,
            )
            .unwrap();
        fixture
            .b
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                None,
                None,
                Some(true),
                None,
                None,
                None,
            )
            .unwrap();
        fixture
            .b
            .vaults
            .update_item_secret(
                fixture.vault_id,
                fixture.item_id,
                "password",
                Some("rotated-on-b".to_string()),
            )
            .unwrap();

        let (a_outcome, b_outcome) = fixture.sync();

        assert!(a_outcome.report.conflicts.is_empty());
        assert!(b_outcome.report.conflicts.is_empty());
        for device in [&fixture.a, &fixture.b] {
            let item = live_item(device, fixture.vault_id, fixture.item_id);
            assert_eq!(item.display_name.as_deref(), Some("Mail (work)"));
            assert!(item.favourite, "B's favourite must travel");
            assert!(item.tags.contains(&"work".to_string()));
            assert_eq!(
                device
                    .vaults
                    .get_item_secret(fixture.vault_id, fixture.item_id, "password")
                    .unwrap()
                    .as_deref(),
                Some("rotated-on-b")
            );
        }
    }

    #[test]
    fn test_a_conflict_prompt_names_both_devices_and_withholds_secret_values() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // Two contested fields: display_name and password. Both Devices also
        // make clean edits that must still be saved.
        for (device, name, username, email, password) in [
            (
                &mut fixture.a,
                "A-name",
                "alice-a",
                None,
                "pw-a",
            ),
            (
                &mut fixture.b,
                "B-name",
                "alice",
                Some("b@example.com"),
                "pw-b",
            ),
        ] {
            device
                .vaults
                .update_item(
                    fixture.vault_id,
                    fixture.item_id,
                    Some(name.to_string()),
                    None,
                    None,
                    None,
                    None,
                    Some(NewItemData::Account(AccountData {
                        username: username.to_string(),
                        email: email.map(str::to_string),
                    })),
                )
                .unwrap();
            device
                .vaults
                .update_item_secret(
                    fixture.vault_id,
                    fixture.item_id,
                    "password",
                    Some(password.to_string()),
                )
                .unwrap();
        }

        let (a_outcome, b_outcome) = fixture.sync();

        assert_eq!(a_outcome.report.conflicts.len(), 2);
        assert_eq!(b_outcome.report.conflicts.len(), 2);

        let a_id = fixture.a.user.device_id();
        let b_id = fixture.b.user.device_id();
        let display = a_outcome
            .report
            .conflicts
            .iter()
            .find(|conflict| conflict.conflict.field_key == "display_name")
            .expect("the display-name disagreement is surfaced");
        assert!(!display.conflict.is_secret);
        let writers: BTreeSet<DeviceId> = display
            .conflict
            .sides
            .iter()
            .map(|side| side.device)
            .collect();
        assert_eq!(
            writers,
            BTreeSet::from([a_id, b_id]),
            "a conflict names the Device that wrote each side"
        );
        assert!(
            display.conflict.sides.iter().all(|side| side.value.is_some()),
            "a non-secret conflict must show both candidate values"
        );

        let password = a_outcome
            .report
            .conflicts
            .iter()
            .find(|conflict| conflict.conflict.field_key == "password")
            .expect("the password disagreement is surfaced");
        assert!(password.conflict.is_secret);
        assert!(
            password
                .conflict
                .sides
                .iter()
                .all(|side| side.value.is_none()),
            "neither secret candidate may reach the prompt"
        );

        // Clean edits were saved even while those two fields conflict.
        for device in [&fixture.a, &fixture.b] {
            let item = live_item(device, fixture.vault_id, fixture.item_id);
            assert_eq!(account_data(item).username, "alice-a");
            assert_eq!(account_data(item).email.as_deref(), Some("b@example.com"));
        }

        // The report itself, serialized for the UI, carries no secret value.
        let wire = serde_json::to_string(&a_outcome.report).unwrap();
        assert!(!wire.contains("pw-a") && !wire.contains("pw-b"), "{wire}");
    }

    #[test]
    fn test_resolving_a_conflict_on_one_device_converges_without_a_second_prompt() {
        let mut fixture = Fixture::new();
        fixture.sync();

        fixture
            .a
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("A-name".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();
        fixture
            .b
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("B-name".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let (a_outcome, _) = fixture.sync();
        let display = pending_for(&a_outcome, "display_name");
        fixture
            .a
            .vaults
            .resolve_conflict_field(
                fixture.vault_id,
                &display.scope,
                &display.field_key,
                display.is_secret,
                display.local_version,
                display.remote.as_ref(),
            )
            .unwrap();

        let (a_second, b_second) = fixture.sync();

        assert!(
            a_second.report.conflicts.is_empty() && b_second.report.conflicts.is_empty(),
            "the choice must settle the field, not prompt the other Device"
        );
        for device in [&fixture.a, &fixture.b] {
            assert_eq!(
                live_item(device, fixture.vault_id, fixture.item_id)
                    .display_name
                    .as_deref(),
                Some("B-name")
            );
        }
    }

    #[test]
    fn test_two_lookalike_items_are_suggested_only_after_a_sync_and_keeping_both_silences_both() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // The same login, created independently on each Device. Same kind,
        // same normalised name, same username: one Duplicate pair once the
        // two copies meet.
        let a_item = match fixture
            .a
            .vaults
            .add_item(
                fixture.vault_id,
                Some("Bank".to_string()),
                None,
                NewItemData::Account(AccountData {
                    username: "bob".to_string(),
                    email: None,
                }),
                Some(vec![SecretInput {
                    key: "password".to_string(),
                    value: "made-on-a".to_string(),
                }]),
            )
            .unwrap()
        {
            crate::SafeItem::Account(account) => account.id,
            other => panic!("expected an account, got {other:?}"),
        };
        let b_item = match fixture
            .b
            .vaults
            .add_item(
                fixture.vault_id,
                Some(" bank ".to_string()),
                None,
                NewItemData::Account(AccountData {
                    username: "bob".to_string(),
                    email: None,
                }),
                Some(vec![SecretInput {
                    key: "password".to_string(),
                    value: "made-on-b".to_string(),
                }]),
            )
            .unwrap()
        {
            crate::SafeItem::Account(account) => account.id,
            other => panic!("expected an account, got {other:?}"),
        };

        // Before the exchange each Device knows only its own copy.
        assert!(fixture
            .a
            .vaults
            .duplicate_suggestions(fixture.vault_id)
            .unwrap()
            .is_empty());
        assert!(fixture
            .b
            .vaults
            .duplicate_suggestions(fixture.vault_id)
            .unwrap()
            .is_empty());

        fixture.sync();

        let suggestion = fixture
            .a
            .vaults
            .duplicate_suggestions(fixture.vault_id)
            .unwrap()
            .remove(0);
        assert_eq!(
            BTreeSet::from([suggestion.first.item_id, suggestion.second.item_id]),
            BTreeSet::from([a_item, b_item])
        );
        assert_eq!(
            fixture
                .b
                .vaults
                .duplicate_suggestions(fixture.vault_id)
                .unwrap()
                .len(),
            1,
            "both Devices see the same suggestion"
        );

        fixture
            .a
            .vaults
            .resolve_duplicate(
                fixture.vault_id,
                a_item,
                b_item,
                crate::DuplicateResolution::KeepBoth,
            )
            .unwrap();

        fixture.sync();

        assert!(fixture
            .a
            .vaults
            .duplicate_suggestions(fixture.vault_id)
            .unwrap()
            .is_empty());
        assert!(
            fixture
                .b
                .vaults
                .duplicate_suggestions(fixture.vault_id)
                .unwrap()
                .is_empty(),
            "one decision must silence the prompt on every Device"
        );
        for device in [&fixture.a, &fixture.b] {
            assert!(
                device
                    .vaults
                    .get_vault(fixture.vault_id)
                    .unwrap()
                    .is_duplicate_dismissed(a_item, b_item)
            );
            assert_eq!(
                device
                    .vaults
                    .get_vault(fixture.vault_id)
                    .unwrap()
                    .items
                    .iter()
                    .filter(|item| item.id == a_item || item.id == b_item)
                    .filter(|item| !item.is_tombstone())
                    .count(),
                2,
                "KeepBoth must keep both identities"
            );
        }
    }

    #[test]
    fn test_combining_duplicates_with_an_explicit_secret_choice_survives_a_sync() {
        let mut fixture = Fixture::new();
        fixture.sync();

        let a_item = match fixture
            .a
            .vaults
            .add_item(
                fixture.vault_id,
                Some("Bank".to_string()),
                None,
                NewItemData::Account(AccountData {
                    username: "bob".to_string(),
                    email: None,
                }),
                Some(vec![SecretInput {
                    key: "password".to_string(),
                    value: "a-bank-password".to_string(),
                }]),
            )
            .unwrap()
        {
            crate::SafeItem::Account(account) => account.id,
            other => panic!("expected an account, got {other:?}"),
        };
        let b_item = match fixture
            .b
            .vaults
            .add_item(
                fixture.vault_id,
                Some("Bank".to_string()),
                None,
                NewItemData::Account(AccountData {
                    username: "bob".to_string(),
                    email: None,
                }),
                Some(vec![SecretInput {
                    key: "password".to_string(),
                    value: "b-bank-password".to_string(),
                }]),
            )
            .unwrap()
        {
            crate::SafeItem::Account(account) => account.id,
            other => panic!("expected an account, got {other:?}"),
        };

        fixture.sync();

        let suggestion = fixture
            .b
            .vaults
            .duplicate_suggestions(fixture.vault_id)
            .unwrap()
            .remove(0);
        let field_choices: BTreeMap<String, ItemId> = suggestion
            .colliding_fields
            .iter()
            .map(|field| (field.field_key.clone(), a_item))
            .collect();
        assert!(
            field_choices.contains_key("password"),
            "the password difference must require an explicit choice"
        );

        fixture
            .b
            .vaults
            .resolve_duplicate(
                fixture.vault_id,
                a_item,
                b_item,
                crate::DuplicateResolution::Combine {
                    survivor: a_item,
                    field_choices,
                },
            )
            .unwrap();

        fixture.sync();

        for device in [&fixture.a, &fixture.b] {
            let vault = device.vaults.get_vault(fixture.vault_id).unwrap();
            let live: Vec<_> = vault
                .items
                .iter()
                .filter(|item| item.id == a_item || item.id == b_item)
                .collect();
            assert_eq!(live.len(), 2, "the vault keeps identity history");
            assert!(
                live.iter().any(|item| item.id == a_item && !item.is_tombstone()),
                "the survivor stays live"
            );
            assert!(
                live.iter()
                    .any(|item| item.id == b_item && item.is_tombstone()),
                "the discarded identity becomes a Tombstone"
            );
            assert_eq!(
                device
                    .vaults
                    .get_item_secret(fixture.vault_id, a_item, "password")
                    .unwrap()
                    .as_deref(),
                Some("a-bank-password"),
                "Combine must carry the chosen secret across the sync"
            );
        }
    }

    // ---- Failure paths ----

    /// Rewrite a travelling registry so one Device's record says something
    /// its admission signature never covered. No honest manager produces
    /// this; the seam is the only place a peer could.
    fn forged_registries(
        registries: &crate::SharedRegistries,
        device: DeviceId,
    ) -> crate::SharedRegistries {
        let mut value = serde_json::to_value(registries).unwrap();
        let records = value
            .get_mut("devices")
            .and_then(|devices| devices.get_mut("devices"))
            .and_then(|records| records.as_object_mut())
            .expect("a registry serializes as a device map");
        let record = records
            .get_mut(&device.to_string())
            .expect("the Device under test is in its own registry");
        record["name"] = serde_json::Value::String("Attacker's Laptop".to_string());
        serde_json::from_value(value).expect("the forged registry still parses")
    }

    struct TamperingNetwork<'a> {
        inner: &'a InMemoryNetwork,
        victim: DeviceId,
    }

    impl PeerNetwork for TamperingNetwork<'_> {
        fn start_advertising(&self) -> Result<(), NetworkError> {
            self.inner.start_advertising()
        }

        fn stop_advertising(&self) -> Result<(), NetworkError> {
            self.inner.stop_advertising()
        }

        fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
            self.inner.discover()
        }

        fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
            Ok(Box::new(TamperingChannel {
                inner: self.inner.connect(peer)?,
                victim: self.victim,
            }))
        }

        fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
            Ok(self.inner.accept()?.map(|inner| {
                Box::new(TamperingChannel {
                    inner,
                    victim: self.victim,
                }) as Box<dyn PeerChannel>
            }))
        }
    }

    struct TamperingChannel {
        inner: Box<dyn PeerChannel>,
        victim: DeviceId,
    }

    impl PeerChannel for TamperingChannel {
        fn peer_device_id(&self) -> DeviceId {
            self.inner.peer_device_id()
        }

        fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
            let message = match message {
                SyncMessage::Scope(mut scope) => {
                    scope.registries = forged_registries(&scope.registries, self.victim);
                    SyncMessage::Scope(scope)
                }
                other => other,
            };
            self.inner.send(message)
        }

        fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
            self.inner.receive()
        }

        fn receive_timeout(
            &mut self,
            timeout: std::time::Duration,
        ) -> Result<SyncMessage, NetworkError> {
            self.inner.receive_timeout(timeout)
        }

        fn close(&mut self) {
            self.inner.close()
        }
    }

    #[test]
    fn test_a_tampered_registry_entry_is_refused_before_any_vault_content_is_written() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // B would have changed this; if the tampered scope were trusted, the
        // exchange would go on to deliver it.
        fixture
            .b
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("should never cross".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let peer = discovered_b(&fixture);
        let tampering = TamperingNetwork {
            inner: &fixture.b.net,
            victim: fixture.b.user.device_id(),
        };
        let mut noop = |_: SyncProgress| {};

        let mut engine_a = device_engine!(fixture, a);
        let start = engine_a.start_with_peer(&peer, &mut noop).unwrap();
        let _accepted = {
            let mut engine_b = SyncEngine::new(
                &tampering,
                &mut fixture.b.vaults,
                &fixture.b.user,
                fixture.b.sync.scope(&fixture.b.user_id),
                SyncCredentials {
                    master_password: MASTER_PW,
                    secret_key: &fixture.b.secret_key,
                },
            );
            engine_b.accept(&mut noop).unwrap().unwrap()
        };

        assert!(matches!(
            engine_a.send_offers(start, &mut noop),
            Err(VaultError::DeviceRegistry(_))
        ));
        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("Mail"),
            "a refused registry must stop the vault exchange"
        );
        assert_eq!(
            account_data(live_item(&fixture.a, fixture.vault_id, fixture.item_id)).username,
            "alice"
        );
    }

    #[test]
    fn test_an_external_file_change_stops_a_sync_instead_of_being_overwritten() {
        let mut fixture = Fixture::new();
        fixture.sync();

        // Keep B's edit in memory so the next save is the one a sync forces.
        fixture.b.vaults.set_autosave(false);
        fixture
            .b
            .vaults
            .update_item(
                fixture.vault_id,
                fixture.item_id,
                Some("not yet on disk".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let path = fixture
            .b
            .dir
            .join("vaults")
            .join(format!("{}.vault", fixture.vault_id));
        let externally_written = b"written by another program".to_vec();
        std::fs::write(&path, &externally_written).unwrap();

        let peer = discovered_b(&fixture);
        let mut noop = |_: SyncProgress| {};
        let _start = {
            let mut engine_a = device_engine!(fixture, a);
            engine_a.start_with_peer(&peer, &mut noop).unwrap()
        };
        let accepted = {
            let mut engine_b = device_engine!(fixture, b);
            engine_b.accept(&mut noop)
        };

        assert!(matches!(accepted, Err(VaultError::ExternalChange(_))));
        assert_eq!(
            std::fs::read(&path).unwrap(),
            externally_written,
            "a sync must never blind-overwrite a file changed underneath it"
        );
        assert_eq!(
            live_item(&fixture.a, fixture.vault_id, fixture.item_id)
                .display_name
                .as_deref(),
            Some("Mail"),
            "the refused sync changes nothing on the other Device"
        );
    }

    /// Captures every frame A sends, so the test can prove what did and did
    /// not travel.
    struct RecordingNetwork<'a> {
        inner: &'a InMemoryNetwork,
        frames: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl PeerNetwork for RecordingNetwork<'_> {
        fn start_advertising(&self) -> Result<(), NetworkError> {
            self.inner.start_advertising()
        }

        fn stop_advertising(&self) -> Result<(), NetworkError> {
            self.inner.stop_advertising()
        }

        fn discover(&self) -> Result<Vec<DiscoveredPeer>, NetworkError> {
            self.inner.discover()
        }

        fn connect(&self, peer: &DiscoveredPeer) -> Result<Box<dyn PeerChannel>, NetworkError> {
            Ok(Box::new(RecordingChannel {
                inner: self.inner.connect(peer)?,
                frames: Arc::clone(&self.frames),
            }))
        }

        fn accept(&self) -> Result<Option<Box<dyn PeerChannel>>, NetworkError> {
            Ok(self.inner.accept()?.map(|inner| {
                Box::new(RecordingChannel {
                    inner,
                    frames: Arc::clone(&self.frames),
                }) as Box<dyn PeerChannel>
            }))
        }
    }

    struct RecordingChannel {
        inner: Box<dyn PeerChannel>,
        frames: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl PeerChannel for RecordingChannel {
        fn peer_device_id(&self) -> DeviceId {
            self.inner.peer_device_id()
        }

        fn send(&mut self, message: SyncMessage) -> Result<(), NetworkError> {
            self.frames
                .lock()
                .unwrap()
                .push(crate::network::encode_frame(&message)?);
            self.inner.send(message)
        }

        fn receive(&mut self) -> Result<SyncMessage, NetworkError> {
            self.inner.receive()
        }

        fn receive_timeout(
            &mut self,
            timeout: std::time::Duration,
        ) -> Result<SyncMessage, NetworkError> {
            self.inner.receive_timeout(timeout)
        }

        fn close(&mut self) {
            self.inner.close()
        }
    }

    #[test]
    fn test_a_sync_carries_no_master_password_and_no_vault_plaintext() {
        let mut fixture = Fixture::new();
        fixture.sync();

        let marker = format!("PLAINTEXT-MARKER-{}", Uuid::new_v4());
        let secret_marker = format!("SECRET-MARKER-{}", Uuid::new_v4());
        fixture
            .a
            .vaults
            .update_vault(fixture.vault_id, Some(marker.clone()), None)
            .unwrap();
        fixture
            .a
            .vaults
            .update_item_secret(
                fixture.vault_id,
                fixture.item_id,
                "password",
                Some(secret_marker.clone()),
            )
            .unwrap();

        let frames = Arc::new(Mutex::new(Vec::new()));
        let recording = RecordingNetwork {
            inner: &fixture.a.net,
            frames: Arc::clone(&frames),
        };
        let peer = discovered_b(&fixture);
        let mut noop = |_: SyncProgress| {};

        let mut engine_a = SyncEngine::new(
            &recording,
            &mut fixture.a.vaults,
            &fixture.a.user,
            fixture.a.sync.scope(&fixture.a.user_id),
            SyncCredentials {
                master_password: MASTER_PW,
                secret_key: &fixture.a.secret_key,
            },
        );
        let mut engine_b = device_engine!(fixture, b);

        let start = engine_a.start_with_peer(&peer, &mut noop).unwrap();
        let accepted = engine_b.accept(&mut noop).unwrap().unwrap();
        let sent = engine_a.send_offers(start, &mut noop).unwrap();
        engine_b.receive_offers(accepted, &mut noop).unwrap();
        engine_a.collect(sent, &mut noop).unwrap();

        let frames = frames.lock().unwrap();
        assert!(!frames.is_empty(), "the exchange must actually occur");
        assert!(frames.iter().all(|frame| frame.starts_with(b"LKRS")));
        let wire = frames
            .iter()
            .map(|frame| String::from_utf8_lossy(frame).into_owned())
            .collect::<String>();

        for forbidden in [MASTER_PW, marker.as_str(), secret_marker.as_str()] {
            assert!(
                !wire.contains(forbidden),
                "the wire must not carry {forbidden:?}"
            );
        }
    }

    #[test]
    fn test_a_sync_reports_progress_from_start_to_complete() {
        let mut fixture = Fixture::new();
        let peer = discovered_b(&fixture);

        let mut a_events = Vec::new();
        let mut b_events = Vec::new();
        let mut a_progress = |event: SyncProgress| a_events.push(event);
        let mut b_progress = |event: SyncProgress| b_events.push(event);

        let mut engine_a = device_engine!(fixture, a);
        let start = engine_a.start_with_peer(&peer, &mut a_progress).unwrap();
        let mut engine_b = device_engine!(fixture, b);
        let accepted = engine_b.accept(&mut b_progress).unwrap().unwrap();
        let sent = engine_a.send_offers(start, &mut a_progress).unwrap();
        engine_b.receive_offers(accepted, &mut b_progress).unwrap();
        engine_a.collect(sent, &mut a_progress).unwrap();

        assert!(matches!(
            a_events.first(),
            Some(SyncProgress::Started { .. })
        ));
        assert!(a_events
            .iter()
            .any(|event| matches!(event, SyncProgress::SendingVault { .. })));
        assert!(matches!(
            a_events.last(),
            Some(SyncProgress::Complete {
                vaults: 1,
                conflicts: 0
            })
        ));

        assert!(b_events
            .iter()
            .any(|event| matches!(event, SyncProgress::ReceivingVault { .. })));
        assert!(b_events
            .iter()
            .any(|event| matches!(event, SyncProgress::VaultMerged { .. })));
        assert!(matches!(
            b_events.last(),
            Some(SyncProgress::Complete {
                vaults: 1,
                conflicts: 0
            })
        ));
    }

    #[test]
    fn test_an_opposite_move_across_two_devices_is_surfaced_as_a_duplicate_note() {
        let mut fixture = Fixture::new();
        fixture.sync();

        let archive = fixture
            .a
            .vaults
            .create_vault(
                "Archive".to_string(),
                "#111111".to_string(),
                MASTER_PW,
                &fixture.a.secret_key,
            )
            .unwrap();
        fixture.a.user.add_vault_to_user(archive.id).unwrap();
        let holding = fixture
            .a
            .vaults
            .create_vault(
                "Holding".to_string(),
                "#222222".to_string(),
                MASTER_PW,
                &fixture.a.secret_key,
            )
            .unwrap();
        fixture.a.user.add_vault_to_user(holding.id).unwrap();
        fixture.sync();
        assert!(fixture.b.vaults.has_vault_file(archive.id));
        assert!(fixture.b.vaults.has_vault_file(holding.id));

        // One identity, moved to two different destinations concurrently.
        fixture
            .a
            .vaults
            .move_item(fixture.vault_id, fixture.item_id, archive.id)
            .unwrap();
        fixture
            .b
            .vaults
            .move_item(fixture.vault_id, fixture.item_id, holding.id)
            .unwrap();

        let (a_outcome, b_outcome) = fixture.sync();

        let notes: Vec<_> = a_outcome
            .report
            .duplicate_notes
            .iter()
            .chain(b_outcome.report.duplicate_notes.iter())
            .filter(|note| note.item_id == fixture.item_id)
            .collect();
        assert!(
            !notes.is_empty(),
            "whole-file, per-Vault merge cannot reconcile opposite moves; the \
             surviving identity must be surfaced, not claimed converged"
        );
        assert!(notes
            .iter()
            .any(|note| note.vault_ids.contains(&archive.id) && note.vault_ids.contains(&holding.id)));

        for device in [&fixture.a, &fixture.b] {
            for destination in [archive.id, holding.id] {
                assert!(
                    device
                        .vaults
                        .get_vault(destination)
                        .unwrap()
                        .items
                        .iter()
                        .any(|item| item.id == fixture.item_id && !item.is_tombstone()),
                    "the limitation leaves a live copy in each destination"
                );
            }
        }
    }
}
