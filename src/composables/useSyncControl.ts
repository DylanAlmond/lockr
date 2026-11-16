import { computed, ref, watch, type Ref } from 'vue';
import { useSync } from './useSync';
import type { ConflictChoice, PairedDevice, SyncConflict, SyncProgress } from '../types';

interface SyncControlOptions {
  devices: Ref<PairedDevice[]>;
  deviceNames: Ref<Record<string, string>>;
  refreshDevices: () => Promise<void>;
  /** Refresh the data a clean merge or a closed Conflict can change. */
  onSynced: () => Promise<void>;
}

/**
 * The one sync run the Devices screen may start, plus the prompts it owns.
 *
 * Sync progress is process-wide (a sync can outlive this screen), so this
 * composable reads and writes the shared `useSync` state rather than keeping
 * a local copy.
 */
export function useSyncControl(options: SyncControlOptions) {
  const {
    isSyncing,
    syncingPeerId,
    progress,
    syncFailure,
    conflicts,
    moveResidueNotes,
    conflictError,
    isResolvingConflict,
    clearSyncFailure,
    syncWithDevice,
    cancelSync,
    onSyncProgress,
    resolveSyncConflict,
    abandonConflicts
  } = useSync();

  const navigationBlocked = ref<string | null>(null);
  const lastAttemptPeerId = ref<string | null>(null);

  const activePeerLabel = computed(() => {
    if (!syncingPeerId.value) return null;
    return (
      options.deviceNames.value[syncingPeerId.value] ??
      `Device ${syncingPeerId.value.slice(0, 8)}`
    );
  });

  async function syncNow(device: PairedDevice) {
    navigationBlocked.value = null;
    lastAttemptPeerId.value = device.device_id;

    const report = await syncWithDevice(device.device_id);
    await options.refreshDevices();

    if (report && report.conflicts.length === 0) {
      await options.onSynced();
    }
  }

  function retryLastSync() {
    const peerId = lastAttemptPeerId.value;
    const device = peerId
      ? options.devices.value.find((candidate) => candidate.device_id === peerId)
      : null;
    if (device) {
      void syncNow(device);
    }
  }

  async function cancelRunningSync() {
    await cancelSync();
  }

  async function handleResolveConflict(conflict: SyncConflict, choice: ConflictChoice) {
    const session = conflicts.value;
    if (!session) return;

    await resolveSyncConflict({
      sessionId: session.sessionId,
      vaultId: conflict.vault_id,
      itemId: conflict.item_id ?? null,
      fieldKey: conflict.field_key,
      choice
    });
  }

  function abandonOpenConflicts() {
    abandonConflicts();
    navigationBlocked.value = null;
    void options.onSynced();
  }

  /**
   * A conflict with no local side is refreshed by an explicit, user-started
   * sync with the same peer. There is no automatic retry: a peer that is
   * offline would turn one clear message into a spinner and then a different
   * error.
   */
  async function resyncFromConflict(peerDeviceId: string) {
    const device = options.devices.value.find(
      (candidate) => candidate.device_id === peerDeviceId
    );
    if (!device) return;
    abandonConflicts();
    await syncNow(device);
  }

  async function subscribeProgress(): Promise<() => void> {
    return onSyncProgress((payload: SyncProgress) => {
      progress.value.push(payload);
    });
  }

  /**
   * The route guard's one answer. A running sync or an open Conflict blocks
   * navigation until it is finished or explicitly abandoned.
   */
  function blockNavigation(): string | null {
    if (isSyncing.value) {
      navigationBlocked.value =
        'A sync is running. Wait for it to finish, or cancel it, before leaving this screen.';
      return navigationBlocked.value;
    }

    if (conflicts.value) {
      navigationBlocked.value =
        'A conflict is still open. Resolve it, or choose “Leave unresolved”, before leaving this screen.';
      return navigationBlocked.value;
    }

    return null;
  }

  watch(
    () => conflicts.value,
    (next, previous) => {
      if (!next && previous) {
        navigationBlocked.value = null;
        void options.onSynced();
      }
    }
  );

  watch(
    () => isSyncing.value,
    (running) => {
      if (!running) navigationBlocked.value = null;
    }
  );

  return {
    navigationBlocked,
    lastAttemptPeerId,
    activePeerLabel,
    isSyncing,
    syncingPeerId,
    progress,
    syncFailure,
    conflicts,
    moveResidueNotes,
    conflictError,
    isResolvingConflict,
    clearSyncFailure,
    syncNow,
    retryLastSync,
    cancelRunningSync,
    handleResolveConflict,
    abandonOpenConflicts,
    resyncFromConflict,
    subscribeProgress,
    blockNavigation
  };
}
