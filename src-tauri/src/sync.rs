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
//!                                -> Vault* then Complete
//! responder.receive_offers       <- Vault* then Complete
//!                                -> Vault* then Complete
//! initiator.collect              <- Vault* then Complete
//! ```
//!
//! The initiator sends first at every phase, and the responder replies, so a
//! queue-backed test channel never has to guess when to block.
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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    Conflict, ConflictScope, DeviceId, ItemId, MergeError, MergeOutcome, UserId, Vault, VaultError,
    VaultId,
};

/// Device-local file holding Sync Sets and last-sync timestamps. It is never
/// part of a Vault and never travels; each Device decides its own scope.
const SYNC_STATE_FILE: &str = "sync_state.json";

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

    /// The safe provenance the merge engine produced, reused verbatim in the
    /// surfaced report so the UI can say which Device wrote which side.
    pub(crate) sides: [crate::ConflictSide; 2],
}

#[derive(Debug, Clone)]
pub(crate) struct PendingSync {
    pub(crate) user_id: UserId,
    pub(crate) conflicts: Vec<PendingConflict>,
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
    local_scope: BTreeSet<VaultId>,
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
    intersection: BTreeSet<VaultId>,
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
        }
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

        if shared.devices.is_revoked(&peer.device_id()) {
            return Err(VaultError::SyncPeerRevoked(peer.device_id().to_string()));
        }

        let local_scope = self.sync_set.ids(&shared.vaults);
        let local_scope_ids: Vec<VaultId> = local_scope.iter().copied().collect();

        let mut channel = self
            .network
            .connect(peer)
            .map_err(Self::map_connect_error)?;

        // A dirty vault must travel as it is now, not as it was last saved.
        self.vault_manager.flush_all()?;

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
            local_scope,
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

        self.reject_revoked_peer(start.peer_device_id, &merged_registry.devices)?;
        let intersection = self.intersection(
            &start.local_scope,
            &remote_scope.vault_ids,
            &merged_registry.vaults,
        );

        let agreed: Vec<VaultId> = intersection.iter().copied().collect();
        progress(SyncProgress::ScopeAgreed {
            peer_device_id: start.peer_device_id,
            vault_ids: agreed,
        });

        for vault_id in &intersection {
            self.send_vault(start.channel.as_mut(), *vault_id, progress)?;
        }
        start
            .channel
            .send(SyncMessage::Complete)
            .map_err(Self::map_channel_error)?;

        Ok(SyncSent {
            channel: start.channel,
            peer_device_id: start.peer_device_id,
            intersection,
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
            match sent.channel.receive().map_err(Self::map_channel_error)? {
                SyncMessage::Vault { vault_id, blob } => {
                    self.integrator_check_scope(&sent.intersection, vault_id)?;
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
        self.reject_revoked_peer(
            peer_device_id,
            &self.user_manager.get_shared_profile(&user_id)?.devices,
        )?;

        let remote_scope = self.receive_scope(&mut *channel)?;
        let merged_registry = self.merge_registries(&remote_scope.registries)?;
        self.reject_revoked_peer(peer_device_id, &merged_registry.devices)?;

        self.vault_manager.flush_all()?;

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

        // Capture this Device's copies *before* merging the initiator's
        // offers. Sending the post-merge copy back would make it look
        // causally later than the initiator's own write (it has absorbed the
        // initiator's clock), silently hiding the very conflict the two
        // original copies describe.
        let own_offers: Vec<(VaultId, Vec<u8>)> = accepted
            .intersection
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
            match accepted
                .channel
                .receive()
                .map_err(Self::map_channel_error)?
            {
                SyncMessage::Vault { vault_id, blob } => {
                    self.integrator_check_scope(&accepted.intersection, vault_id)?;
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

        // Responder replies with the pre-merge copies captured above. Both
        // Devices therefore merge the same two original states, compute the
        // same conflicts, and keep their own contested values.
        for (vault_id, bytes) in own_offers {
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

    fn receive_scope(&self, channel: &mut dyn PeerChannel) -> Result<ScopeState, VaultError> {
        match channel.receive().map_err(Self::map_channel_error)? {
            SyncMessage::Scope(scope) => Ok(scope),
            other => Err(VaultError::SyncRefused(format!(
                "expected the peer's scope, got {other:?}"
            ))),
        }
    }

    /// Merge the peer's shared registries into ours. Revocation wins, so a
    /// peer cannot un-revoke a Device by presenting an older copy.
    fn merge_registries(&self, remote: &SharedRegistries) -> Result<SharedRegistries, VaultError> {
        let user_id = self
            .user_manager
            .active_user_id
            .ok_or(VaultError::SyncNotLoggedIn)?;
        let mut shared = self.user_manager.get_shared_profile(&user_id)?;

        shared.devices = shared.devices.merge(&remote.devices)?;
        for vault_id in remote.vaults.iter() {
            shared.vaults.insert(*vault_id);
        }

        if shared.devices.is_revoked(&self.vault_manager.device_id()) {
            return Err(VaultError::SyncThisDeviceRevoked(
                self.vault_manager.device_id().to_string(),
            ));
        }

        self.user_manager.save_shared_profile(&shared)?;
        Ok(shared.registries())
    }

    fn reject_revoked_peer(
        &self,
        peer_device_id: DeviceId,
        devices: &crate::DeviceRegistry,
    ) -> Result<(), VaultError> {
        if devices.is_revoked(&peer_device_id) {
            Err(VaultError::SyncPeerRevoked(peer_device_id.to_string()))
        } else {
            Ok(())
        }
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
            NetworkError::MalformedFrame(detail) => VaultError::SyncRefused(detail),
        }
    }

    fn map_channel_error(err: NetworkError) -> VaultError {
        match err {
            NetworkError::ConnectionClosed => {
                VaultError::SyncInterrupted("the peer channel closed".into())
            }
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
    use crate::{AccountData, NewItemData, PairingCode, SecretInput};

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
            let outcome_b = engine_b.receive_offers(accepted, &mut noop).unwrap();
            let outcome_a = engine_a.collect(sent, &mut noop).unwrap();

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
}
