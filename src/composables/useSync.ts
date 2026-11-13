import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ConflictChoice,
  ConflictResolutionResult,
  DiscoveredPeer,
  PairingApprovalPrompt,
  PairingCode,
  PairingMessage,
  PairingOffer,
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

/** Classify a backend error string into the UI's distinct failure states. */
export function classifySyncError(error: unknown): SyncFailure {
  const raw = String(error);
  const lower = raw.toLowerCase();

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

  if (lower.includes('device identity is damaged') || lower.includes('identity is damaged')) {
    return failure(
      'damaged-identity',
      'This Device’s identity is damaged',
      'Lockr will not replace this Device’s identity on its own. Restore the identity file from a backup, or start again with a new identity below.',
      false,
      true
    );
  }

  if (raw.startsWith('This Device has been revoked')) {
    return failure(
      'this-device-revoked',
      'This Device has been revoked',
      'No honest Device will sync with this copy any more. Pair this installation again from another Device to rejoin with a new identity.',
      false
    );
  }

  if (raw.startsWith('Sync peer has been revoked')) {
    return failure(
      'revoked',
      'That Device has been revoked',
      'Revocation is permanent. This Device will not sync with it again, even if its old copy of the Device list disagrees.',
      false
    );
  }

  if (raw.startsWith('Sync peer is unreachable')) {
    return failure(
      'unreachable',
      'That Device could not be reached',
      'Check that the other Device is running Lockr on the same network, then try again. Nothing was changed.',
      true
    );
  }

  if (raw.startsWith('Sync refused: Device ') && raw.includes('not paired')) {
    return failure(
      'unpaired',
      'That Device is not paired with this one',
      'Pairing is the only grant of access. Admit the Device from the Devices screen before syncing with it.',
      false
    );
  }

  if (
    raw.startsWith('Cannot sync with a Device using vault schema') ||
    raw.startsWith('Incompatible vault schema')
  ) {
    return failure(
      'schema-mismatch',
      'The two Devices use different vault schemas',
      'Update the older Device. Lockr will not send data the other Device cannot read.',
      false
    );
  }

  if (raw.startsWith('Sync was cancelled')) {
    return failure(
      'cancelled',
      'Sync cancelled',
      'Any Vault that finished transferring is safe to open. Everything else was left as it was before the sync.',
      true
    );
  }

  if (raw.startsWith('Sync was interrupted')) {
    return failure(
      'interrupted',
      'The connection dropped mid-sync',
      'Every Vault is still readable and the sync is safe to retry. A complete Vault is the smallest unit transferred.',
      true
    );
  }

  if (raw.startsWith('Sync timed out')) {
    return failure('timeout', 'The other Device stopped responding', 'Try again once it is awake and on the network.', true);
  }

  if (raw.startsWith('Sync discovery failed')) {
    return failure(
      'discovery',
      'Devices could not be discovered',
      'Discovery needs the local network. Check the connection and try refreshing the list.',
      true
    );
  }

  if (raw.startsWith('Sync transport is unavailable')) {
    return failure(
      'transport',
      'The secure sync channel is unavailable',
      'This build cannot open a channel to another Device. No vault data was sent.',
      false
    );
  }

  if (raw.startsWith('Sync authentication failed')) {
    return failure(
      'authentication',
      'The other Device could not be authenticated',
      'The secure channel did not prove which Device answered. Re-pairing is the fallback.',
      false
    );
  }

  if (raw.startsWith('Sync refused: no User')) {
    return failure(
      'not-logged-in',
      'Sign in to sync',
      'Sync uses the credentials already resident in this session. Sign in and try again.',
      false
    );
  }

  if (raw.includes('already in progress')) {
    return failure(
      'busy',
      'A sync is already running',
      'Only one sync runs at a time on this Device. Wait for it to finish, or cancel it first.',
      false
    );
  }

  return failure('unknown', 'The sync did not complete', 'Nothing was reported as complete. Try again, or check the other Device.', true);
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

  // ============ Pairing ============

  async function startPairing(): Promise<PairingOffer | null> {
    error.value = null;
    try {
      return await invoke<PairingOffer>('start_pairing');
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function cancelPairing(): Promise<boolean> {
    try {
      await invoke('cancel_pairing');
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function joinPairing(code: PairingCode): Promise<PairingMessage | null> {
    error.value = null;
    try {
      return await invoke<PairingMessage>('join_pairing', { code });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function receivePairingRequest(
    message: PairingMessage
  ): Promise<PairingApprovalPrompt | null> {
    error.value = null;
    try {
      return await invoke<PairingApprovalPrompt>('receive_pairing_request', { message });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  /**
   * Returns the transfer frame. It carries the User's secret key and is
   * deliberately never rendered or stored in display state: the caller only
   * hands it straight to the authenticated pairing channel.
   */
  async function approvePairing(
    deviceId: string,
    syncSet: string[] | null
  ): Promise<PairingMessage | null> {
    error.value = null;
    try {
      return await invoke<PairingMessage>('approve_pairing', { deviceId, syncSet });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  /**
   * Joining Device: re-wrap the transferred secret key under the password
   * typed on this Device. The frame never reaches display state.
   */
  async function completePairing(
    message: PairingMessage,
    masterPassword: string
  ): Promise<User | null> {
    error.value = null;
    try {
      return await invoke<User>('complete_pairing', { message, masterPassword });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  // ============ Devices and discovery ============

  async function listPairedDevices(): Promise<PairedDevice[] | null> {
    try {
      return await invoke<PairedDevice[]>('list_paired_devices');
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function startDiscovery(): Promise<boolean> {
    try {
      await invoke('start_discovery');
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function stopDiscovery(): Promise<boolean> {
    try {
      await invoke('stop_discovery');
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function listDiscoveredPeers(): Promise<DiscoveredPeer[] | null> {
    try {
      return await invoke<DiscoveredPeer[]>('list_discovered_peers');
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function revokeDevice(deviceId: string): Promise<boolean> {
    error.value = null;
    try {
      await invoke('revoke_device', { deviceId });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
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
    try {
      await invoke('cancel_sync');
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
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
    try {
      return await invoke<SyncSetView>('get_sync_set');
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  /** Replace the Sync Set with this Device's explicit choice. */
  async function setSyncSet(vaultIds: string[]): Promise<boolean> {
    error.value = null;
    try {
      await invoke('set_sync_set', { vaultIds });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  /** Toggle one Vault without replacing the whole set. Off keeps the copy. */
  async function setVaultSyncing(vaultId: string, syncing: boolean): Promise<boolean> {
    error.value = null;
    try {
      await invoke('set_vault_syncing', { vaultId, syncing });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function listUnregisteredVaults(): Promise<UnregisteredVault[] | null> {
    try {
      return await invoke<UnregisteredVault[]>('list_unregistered_vaults');
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  /** Adopt a local `.vault` file. Adoption shares its entire contents. */
  async function adoptVault(vaultId: string): Promise<boolean> {
    error.value = null;
    try {
      await invoke('adopt_vault', { vaultId });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  // ============ Device identity ============

  async function getDeviceIdentityError(): Promise<string | null> {
    try {
      return await invoke<string | null>('get_device_identity_error');
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function startWithNewDeviceIdentity(confirm: boolean): Promise<boolean> {
    error.value = null;
    try {
      await invoke('start_with_new_device_identity', { confirm });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
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

    // Sync
    syncWithDevice,
    cancelSync,
    onSyncProgress,
    resolveSyncConflict,
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
