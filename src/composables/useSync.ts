import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
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

/**
 * The distinct ways a sync can fail. Each one has its own title and advice so
 * the Devices screen never reduces "could not reach the laptop" and "that
 * Device was revoked" to the same red box.
 */
export type SyncFailureKind =
  | 'unreachable'
  | 'revoked'
  | 'unpaired'
  | 'this-device-revoked'
  | 'schema-mismatch'
  | 'damaged-identity'
  | 'cancelled'
  | 'interrupted'
  | 'timeout'
  | 'transport'
  | 'discovery'
  | 'authentication'
  | 'not-logged-in'
  | 'busy'
  | 'unknown';

export interface SyncFailure {
  kind: SyncFailureKind;
  title: string;
  detail: string;
  advice: string;
  /** True when retrying the same peer is a reasonable next step. */
  canRetry: boolean;
  /** Only a damaged Device identity offers this way out. */
  canRecoverIdentity: boolean;
  raw: string;
}

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

/**
 * A backend failure arrives as `{ code, message }`. The code is the stable
 * discriminator; the message is display text and may be reworded freely.
 */
function syncErrorInfo(error: unknown): { code: string | null; raw: string } {
  if (typeof error === 'object' && error !== null) {
    const candidate = error as { code?: unknown; message?: unknown };
    return {
      code: typeof candidate.code === 'string' ? candidate.code : null,
      raw:
        typeof candidate.message === 'string'
          ? candidate.message
          : JSON.stringify(candidate)
    };
  }

  return { code: null, raw: String(error) };
}

/** Classify a backend sync failure by its stable error code. */
export function classifySyncError(error: unknown): SyncFailure {
  const { code, raw } = syncErrorInfo(error);

  const failure = (
    kind: SyncFailureKind,
    title: string,
    advice: string,
    canRetry = true,
    canRecoverIdentity = false
  ): SyncFailure => ({
    kind,
    title,
    detail: raw,
    advice,
    canRetry,
    canRecoverIdentity,
    raw
  });

  switch (code) {
    case 'device.identity_damaged':
      return failure(
        'damaged-identity',
        'This Device’s identity is damaged',
        'Lockr will not replace this Device’s identity on its own. Restore the identity file from a backup, or start again with a new identity below.',
        false,
        true
      );

    case 'sync.this_device_revoked':
      return failure(
        'this-device-revoked',
        'This Device has been revoked',
        'No honest Device will sync with this copy any more. Pair this installation again from another Device to rejoin with a new identity.',
        false
      );

    case 'sync.device_revoked':
      return failure(
        'revoked',
        'That Device has been revoked',
        'Revocation is permanent. This Device will not sync with it again, even if its old copy of the Device list disagrees.',
        false
      );

    case 'sync.device_unreachable':
      return failure(
        'unreachable',
        'That Device could not be reached',
        'Check that the other Device is running Lockr on the same network, then try again. Nothing was changed.',
        true
      );

    case 'sync.device_not_paired':
      return failure(
        'unpaired',
        'That Device is not paired with this one',
        'Pairing is the only grant of access. Admit the Device from the Devices screen before syncing with it.',
        false
      );

    case 'sync.schema_mismatch':
    case 'pairing.schema_mismatch':
      return failure(
        'schema-mismatch',
        'The two Devices use different vault schemas',
        'Update the older Device. Lockr will not send data the other Device cannot read.',
        false
      );

    case 'sync.cancelled':
      return failure(
        'cancelled',
        'Sync cancelled',
        'Any Vault that finished transferring is safe to open. Everything else was left as it was before the sync.',
        true
      );

    case 'sync.interrupted':
      return failure(
        'interrupted',
        'The connection dropped mid-sync',
        'Every Vault is still readable and the sync is safe to retry. A complete Vault is the smallest unit transferred.',
        true
      );

    case 'sync.timeout':
      return failure(
        'timeout',
        'The other Device stopped responding',
        'Try again once it is awake and on the network.',
        true
      );

    case 'sync.discovery_failed':
      return failure(
        'discovery',
        'Devices could not be discovered',
        'Discovery needs the local network. Check the connection and try refreshing the list.',
        true
      );

    case 'sync.transport_unavailable':
      return failure(
        'transport',
        'The secure sync channel is unavailable',
        'This build cannot open a channel to another Device. No vault data was sent.',
        false
      );

    case 'sync.authentication_failed':
      return failure(
        'authentication',
        'The other Device could not be authenticated',
        'The secure channel did not prove which Device answered. Re-pairing is the fallback.',
        false
      );

    case 'sync.not_logged_in':
      return failure(
        'not-logged-in',
        'Sign in to sync',
        'Sync uses the credentials already resident in this session. Sign in and try again.',
        false
      );

    case 'sync.busy':
      return failure(
        'busy',
        'A sync is already running',
        'Only one sync runs at a time on this Device. Wait for it to finish, or cancel it first.',
        false
      );

    default:
      return failure(
        'unknown',
        'The sync did not complete',
        'Nothing was reported as complete. Try again, or check the other Device.',
        true
      );
  }
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
