import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { classifySyncError } from '../util/syncErrors';
import type { SyncFailure } from '../util/syncErrors';
import type {
  ConflictChoice,
  ConflictResolutionResult,
  DiscoveredPeer,
  PairingApprovalPrompt,
  PairingCode,
  PairingJoinReady,
  PairingOffer,
  PairingOutcome,
  PairedDevice,
  SyncConflict,
  SyncProgress,
  SyncReport,
  SyncSetView,
  UnregisteredVault,
  User
} from '../types';

export { classifySyncError } from '../util/syncErrors';
export type { SyncFailure, SyncFailureKind } from '../util/syncErrors';

/** One open sync session whose Conflicts hold it open. */
export interface ConflictSession {
  sessionId: string;
  peerDeviceId: string;
  conflicts: SyncConflict[];
}

export interface ConflictResolutionRequest {
  sessionId: string;
  vaultId: string;
  itemId: string | null;
  fieldKey: string;
  choice: ConflictChoice;
}

/** One on-demand secret reveal. `value: null` means the candidate is not set. */
export interface RevealedConflictValue {
  value: string | null;
}

// Sync progress is process-wide: a sync can outlive the screen that started it.
const isSyncing = ref(false);
const syncingPeerId = ref<string | null>(null);
const progress = ref<SyncProgress[]>([]);
const syncFailure = ref<SyncFailure | null>(null);
const conflicts = ref<ConflictSession | null>(null);
const duplicateNotes = ref<SyncReport['duplicate_notes']>([]);
const lastReport = ref<SyncReport | null>(null);
const conflictError = ref<string | null>(null);
const error = ref<string | null>(null);

export function useSync() {
  const isResolvingConflict = ref(false);

  function clearSyncFailure() {
    syncFailure.value = null;
  }

  function resetSyncSession() {
    isSyncing.value = false;
    syncingPeerId.value = null;
    progress.value = [];
    syncFailure.value = null;
    conflicts.value = null;
    duplicateNotes.value = [];
    lastReport.value = null;
    conflictError.value = null;
  }

  // ============ Backend calls ============

  /**
   * One command call, one error convention: a failed call records the message
   * and yields null; a successful `void` command yields undefined, which is
   * still a success. Callers that return a boolean wrap this in `requestOk`.
   */
  async function request<T>(
    command: string,
    args?: Record<string, unknown>,
    clearError = false
  ): Promise<T | null> {
    if (clearError) error.value = null;

    try {
      return await invoke<T>(command, args);
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function requestOk(
    command: string,
    args?: Record<string, unknown>,
    clearError = false
  ): Promise<boolean> {
    if (clearError) error.value = null;

    try {
      // A `Result<(), _>` command resolves to `null`, which is a success.
      // This is deliberately separate from `request`, whose `null` means a
      // failed call for commands that return a value.
      await invoke(command, args);
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  // ============ Pairing ============

  async function startPairing(): Promise<PairingOffer | null> {
    return request<PairingOffer>('start_pairing', undefined, true);
  }

  async function cancelPairing(): Promise<boolean> {
    return requestOk('cancel_pairing');
  }

  async function joinPairing(code: PairingCode): Promise<PairingJoinReady | null> {
    return request<PairingJoinReady>('join_pairing', { code }, true);
  }

  async function receivePairingRequest(): Promise<PairingApprovalPrompt | null> {
    return request<PairingApprovalPrompt>('receive_pairing_request', undefined, true);
  }

  /** The transfer stays in the backend; only the outcome is returned. */
  async function approvePairing(
    deviceId: string,
    syncSet: string[] | null
  ): Promise<PairingOutcome | null> {
    return request<PairingOutcome>('approve_pairing', { deviceId, syncSet }, true);
  }

  /**
   * Joining Device: re-wrap the transferred secret key under the password
   * typed on this Device. The transfer never reaches the frontend.
   */
  async function completePairing(masterPassword: string): Promise<User | null> {
    return request<User>('complete_pairing', { masterPassword }, true);
  }

  /** Serve inbound sync. Advertising remains owned by the discovery session. */
  async function startServing(): Promise<boolean> {
    return requestOk('start_serving', undefined, true);
  }

  async function stopServing(): Promise<boolean> {
    return requestOk('stop_serving');
  }

  // ============ Devices and discovery ============

  async function listPairedDevices(): Promise<PairedDevice[] | null> {
    return request<PairedDevice[]>('list_paired_devices');
  }

  async function startDiscovery(): Promise<boolean> {
    return requestOk('start_discovery');
  }

  async function stopDiscovery(): Promise<boolean> {
    return requestOk('stop_discovery');
  }

  async function listDiscoveredPeers(): Promise<DiscoveredPeer[] | null> {
    return request<DiscoveredPeer[]>('list_discovered_peers');
  }

  async function revokeDevice(deviceId: string): Promise<boolean> {
    return requestOk('revoke_device', { deviceId }, true);
  }

  // ============ Sync ============

  /**
   * Sync with one paired Device. Progress arrives on the `sync-progress`
   * event; a returned report with Conflicts keeps this session open until
   * each one is resolved or the User abandons the prompt.
   */
  async function syncWithDevice(deviceId: string): Promise<SyncReport | null> {
    if (isSyncing.value) {
      error.value = 'A sync is already running';
      return null;
    }

    isSyncing.value = true;
    syncingPeerId.value = deviceId;
    progress.value = [];
    syncFailure.value = null;
    conflicts.value = null;
    duplicateNotes.value = [];
    lastReport.value = null;
    conflictError.value = null;

    try {
      const report = await invoke<SyncReport>('sync_with_device', { deviceId });
      lastReport.value = report;
      duplicateNotes.value = report.duplicate_notes;
      if (report.conflicts.length > 0) {
        conflicts.value = {
          sessionId: report.session_id,
          peerDeviceId: report.peer_device_id,
          conflicts: report.conflicts
        };
      }
      return report;
    } catch (e) {
      syncFailure.value = classifySyncError(e);
      return null;
    } finally {
      isSyncing.value = false;
      syncingPeerId.value = null;
    }
  }

  async function cancelSync(): Promise<boolean> {
    return requestOk('cancel_sync');
  }

  function onSyncProgress(handler: (payload: SyncProgress) => void): Promise<UnlistenFn> {
    return listen<SyncProgress>('sync-progress', (event) => handler(event.payload));
  }

  function conflictsOf(session: ConflictSession, conflict: SyncConflict): SyncConflict[] {
    return session.conflicts.filter(
      (candidate) =>
        !(
          candidate.vault_id === conflict.vault_id &&
          candidate.field_key === conflict.field_key &&
          (candidate.item_id ?? null) === (conflict.item_id ?? null)
        )
    );
  }

  /** Choose one side of a field Conflict. The backend writes only on success. */
  async function resolveSyncConflict(
    request: ConflictResolutionRequest
  ): Promise<ConflictResolutionResult | null> {
    isResolvingConflict.value = true;
    conflictError.value = null;

    try {
      const result = await invoke<ConflictResolutionResult>('resolve_sync_conflict', {
        sessionId: request.sessionId,
        vaultId: request.vaultId,
        itemId: request.itemId,
        fieldKey: request.fieldKey,
        choice: request.choice
      });

      const session = conflicts.value;
      if (session) {
        const conflict = session.conflicts.find(
          (candidate) =>
            candidate.vault_id === request.vaultId &&
            candidate.field_key === request.fieldKey &&
            (candidate.item_id ?? null) === (request.itemId ?? null)
        );
        const remaining = conflict ? conflictsOf(session, conflict) : session.conflicts;

        if (result.remaining === 0 || remaining.length === 0) {
          conflicts.value = null;
        } else {
          conflicts.value = { ...session, conflicts: remaining };
        }
      }

      return result;
    } catch (e) {
      conflictError.value = String(e);
      return null;
    } finally {
      isResolvingConflict.value = false;
    }
  }

  /**
   * Reveal one side of a secret Conflict. This is the per-field on-demand
   * fetch: the Conflict payload never carries either value, so a reveal must
   * be asked for explicitly and applies to one field at a time.
   */
  async function revealConflictCandidate(
    request: ConflictResolutionRequest
  ): Promise<RevealedConflictValue | null> {
    conflictError.value = null;

    try {
      const value = await invoke<string | null>('get_conflict_candidate_secret', {
        sessionId: request.sessionId,
        vaultId: request.vaultId,
        itemId: request.itemId,
        fieldKey: request.fieldKey,
        choice: request.choice
      });
      return { value };
    } catch (e) {
      conflictError.value = String(e);
      return null;
    }
  }

  /**
   * Dismiss the prompt without choosing. Clean merges are already saved.
   * The backend has no abandon command, so the session stays open until
   * logout; this only stops the UI blocking navigation.
   */
  function abandonConflicts(): void {
    conflicts.value = null;
    conflictError.value = null;
  }

  // ============ Sync Set ============

  async function getSyncSet(): Promise<SyncSetView | null> {
    return request<SyncSetView>('get_sync_set');
  }

  /** Replace the Sync Set with this Device's explicit choice. */
  async function setSyncSet(vaultIds: string[]): Promise<boolean> {
    return requestOk('set_sync_set', { vaultIds }, true);
  }

  /** Toggle one Vault without replacing the whole set. Off keeps the copy. */
  async function setVaultSyncing(vaultId: string, syncing: boolean): Promise<boolean> {
    return requestOk('set_vault_syncing', { vaultId, syncing }, true);
  }

  async function listUnregisteredVaults(): Promise<UnregisteredVault[] | null> {
    return request<UnregisteredVault[]>('list_unregistered_vaults');
  }

  /** Adopt a local `.vault` file. Adoption shares its entire contents. */
  async function adoptVault(vaultId: string): Promise<boolean> {
    return requestOk('adopt_vault', { vaultId }, true);
  }

  // ============ Device identity ============

  async function getDeviceIdentityError(): Promise<string | null> {
    return request<string | null>('get_device_identity_error');
  }

  async function startWithNewDeviceIdentity(confirm: boolean): Promise<boolean> {
    return requestOk('start_with_new_device_identity', { confirm }, true);
  }

  return {
    error,
    isSyncing,
    syncingPeerId,
    progress,
    syncFailure,
    conflicts,
    duplicateNotes,
    lastReport,
    conflictError,
    isResolvingConflict,

    clearSyncFailure,
    resetSyncSession,

    // Pairing
    startPairing,
    cancelPairing,
    joinPairing,
    receivePairingRequest,
    approvePairing,
    completePairing,

    // Devices and discovery
    listPairedDevices,
    startDiscovery,
    stopDiscovery,
    listDiscoveredPeers,
    revokeDevice,
    startServing,
    stopServing,

    // Sync
    syncWithDevice,
    cancelSync,
    onSyncProgress,
    resolveSyncConflict,
    revealConflictCandidate,
    abandonConflicts,

    // Sync Set
    getSyncSet,
    setSyncSet,
    setVaultSyncing,
    listUnregisteredVaults,
    adoptVault,

    // Device identity
    getDeviceIdentityError,
    startWithNewDeviceIdentity
  };
}
