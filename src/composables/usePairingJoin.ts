import { onUnmounted, ref } from 'vue';
import { useSync } from './useSync';
import useAppStore from '../stores/appStore';
import type { PairingJoinReady } from '../types';

/**
 * The joining side of pairing: enter the code, wait for the source's approval,
 * then wrap the transferred secret key under the password typed here. The
 * transfer itself stays in the backend.
 */
export function usePairingJoin() {
  const {
    error,
    cancelPairing,
    joinPairing,
    completePairing,
    syncWithDevice,
    syncFailure
  } = useSync();
  const { state } = useAppStore();

  const busy = ref(false);
  const localError = ref<string | null>(null);
  const codeMode = ref<'typed' | 'qr'>('typed');
  const sourceDeviceId = ref('');
  const typedCode = ref('');
  const qrPayload = ref('');
  const joinStage = ref<'idle' | 'waiting' | 'approved' | 'done'>('idle');
  const joinReady = ref<PairingJoinReady | null>(null);
  const joinPassword = ref('');
  const joinAborted = ref(false);
  const joinSyncState = ref<'idle' | 'syncing' | 'done' | 'conflicts' | 'failed'>('idle');
  const joinSyncMessage = ref<string | null>(null);

  let disposed = false;

  onUnmounted(() => {
    disposed = true;
    joinAborted.value = true;
    if (joinStage.value === 'waiting' || joinStage.value === 'approved') {
      void cancelPairing();
    }
  });

  async function beginJoin() {
    busy.value = true;
    localError.value = null;
    joinAborted.value = false;
    joinStage.value = 'waiting';
    joinReady.value = null;

    const code =
      codeMode.value === 'qr'
        ? ({ kind: 'qr', payload: qrPayload.value.trim() } as const)
        : ({
            kind: 'typed',
            source_device_id: sourceDeviceId.value.trim(),
            code: typedCode.value.trim()
          } as const);

    const ready = await joinPairing(code);
    busy.value = false;

    if (joinAborted.value || disposed) return;

    if (!ready) {
      joinStage.value = 'idle';
      localError.value = error.value;
      return;
    }

    joinReady.value = ready;
    joinStage.value = 'approved';
  }

  async function finishJoin() {
    busy.value = true;
    localError.value = null;
    joinSyncState.value = 'idle';
    joinSyncMessage.value = null;

    const user = await completePairing(joinPassword.value);

    if (!user) {
      busy.value = false;
      localError.value = error.value;
      return;
    }

    joinPassword.value = '';
    joinStage.value = 'done';

    // Pairing transfers identity and metadata; the Vaults themselves arrive
    // over the ordinary sync engine, so this Device finishes with the Sync
    // Set's Vaults present instead of asking the User to sync manually.
    await runInitialSync();
    busy.value = false;
  }

  async function runInitialSync() {
    const sourceDeviceId = joinReady.value?.source_device_id;
    if (!sourceDeviceId) {
      joinSyncState.value = 'failed';
      joinSyncMessage.value =
        'Pairing completed, but Lockr does not know which Device to fetch the Vaults from.';
      return;
    }

    joinSyncState.value = 'syncing';
    joinSyncMessage.value = null;

    const report = await syncWithDevice(sourceDeviceId);

    if (!report) {
      joinSyncState.value = 'failed';
      joinSyncMessage.value =
        syncFailure.value?.detail ?? error.value ?? 'The Vaults could not be fetched after pairing.';
      return;
    }

    state.vaultMutationCount += 1;

    if (report.conflicts.length > 0) {
      joinSyncState.value = 'conflicts';
      joinSyncMessage.value =
        'Your Vaults are on this Device, but some fields conflict. Resolve them to finish the first sync.';
      return;
    }

    joinSyncState.value = 'done';
    joinSyncMessage.value =
      report.vault_ids.length > 0
        ? `Your Vaults are on this Device and ready to open (${report.vault_ids.length} Vault${report.vault_ids.length === 1 ? '' : 's'}).`
        : 'Pairing is complete. No Vaults were in the agreed Sync Set.';
  }

  async function reset() {
    joinAborted.value = true;
    busy.value = true;
    await cancelPairing();
    busy.value = false;
    joinStage.value = 'idle';
    joinReady.value = null;
    joinPassword.value = '';
    joinSyncState.value = 'idle';
    joinSyncMessage.value = null;
    qrPayload.value = '';
    typedCode.value = '';
    sourceDeviceId.value = '';
    localError.value = null;
  }

  return {
    busy,
    localError,
    codeMode,
    sourceDeviceId,
    typedCode,
    qrPayload,
    joinStage,
    joinReady,
    joinPassword,
    joinSyncState,
    joinSyncMessage,
    beginJoin,
    finishJoin,
    runInitialSync,
    reset
  };
}
