import { computed, onMounted, onUnmounted, ref } from 'vue';
import { useSync } from './useSync';
import { pairingHandoffText } from '../util/pairing';
import type {
  PairingApprovalPrompt,
  PairingOffer,
  PairingOutcome,
  SyncSetVault
} from '../types';

/**
 * The source side of pairing: make a single-use offer, wait for a joining
 * Device, approve it by name/platform, and pre-check the Sync Set it receives.
 */
export function usePairingOffer() {
  const {
    error,
    startPairing,
    cancelPairing,
    receivePairingRequest,
    approvePairing,
    getSyncSet
  } = useSync();

  const busy = ref(false);
  const localError = ref<string | null>(null);
  const offer = ref<PairingOffer | null>(null);
  const vaults = ref<SyncSetVault[]>([]);
  const syncSetLoaded = ref(false);
  const checked = ref<Set<string>>(new Set());
  const waitingForJoin = ref(false);
  const approval = ref<PairingApprovalPrompt | null>(null);
  const outcome = ref<PairingOutcome | null>(null);
  const copied = ref(false);
  const handoffCopied = ref(false);

  const now = ref(Date.now());
  let timer: ReturnType<typeof setInterval> | null = null;
  let disposed = false;

  onMounted(() => {
    timer = setInterval(() => {
      now.value = Date.now();
    }, 1000);
  });

  onUnmounted(() => {
    disposed = true;
    if (timer) clearInterval(timer);
    if (offer.value || waitingForJoin.value || approval.value) {
      void cancelPairing();
    }
  });

  const secondsLeft = computed(() => {
    if (!offer.value) return 0;
    const expiry = new Date(offer.value.expires_at).getTime();
    return Math.max(0, Math.ceil((expiry - now.value) / 1000));
  });

  const offerExpired = computed(() => !!offer.value && secondsLeft.value <= 0);

  function toggleVault(vaultId: string) {
    const next = new Set(checked.value);
    if (next.has(vaultId)) {
      next.delete(vaultId);
    } else {
      next.add(vaultId);
    }
    checked.value = next;
  }

  async function beginOffer() {
    busy.value = true;
    localError.value = null;
    outcome.value = null;
    approval.value = null;

    const created = await startPairing();
    busy.value = false;

    if (!created) {
      localError.value = error.value;
      return;
    }

    offer.value = created;

    const set = await getSyncSet();
    syncSetLoaded.value = !!set;
    vaults.value = set?.vaults ?? [];
    checked.value = new Set(vaults.value.map((vault) => vault.vault_id));

    // The backend owns the transport. This command blocks until a joining
    // Device actually announces itself; the UI never sees a handshake frame.
    void waitForApproval();
  }

  async function waitForApproval() {
    waitingForJoin.value = true;
    localError.value = null;

    const prompt = await receivePairingRequest();
    waitingForJoin.value = false;

    if (disposed || !offer.value) return;

    if (!prompt) {
      localError.value = offerExpired.value
        ? 'The pairing code expired. Start again.'
        : error.value ?? 'The joining Device could not be approved.';
      return;
    }

    approval.value = prompt;
  }

  async function copyPayload() {
    if (!offer.value) return;
    try {
      await navigator.clipboard.writeText(offer.value.qr_payload);
      copied.value = true;
      setTimeout(() => {
        copied.value = false;
      }, 2000);
    } catch {
      localError.value = 'Could not copy the pairing payload.';
    }
  }

  /**
   * Both values the typed join path asks for, in one clipboard entry. The
   * Device id is not otherwise visible outside the QR payload.
   */
  async function copyHandoff() {
    if (!offer.value) return;
    try {
      await navigator.clipboard.writeText(pairingHandoffText(offer.value));
      handoffCopied.value = true;
      setTimeout(() => {
        handoffCopied.value = false;
      }, 2000);
    } catch {
      localError.value = 'Could not copy the pairing code and Device id.';
    }
  }

  async function approve() {
    if (!approval.value) return;

    busy.value = true;
    localError.value = null;
    const result = await approvePairing(
      approval.value.device_id,
      syncSetLoaded.value ? [...checked.value] : null
    );
    busy.value = false;

    if (!result) {
      localError.value = error.value;
      return;
    }

    outcome.value = result;
    approval.value = null;
    offer.value = null;
  }

  async function reset() {
    busy.value = true;
    await cancelPairing();
    busy.value = false;
    offer.value = null;
    approval.value = null;
    outcome.value = null;
    waitingForJoin.value = false;
    localError.value = null;
    syncSetLoaded.value = false;
  }

  return {
    busy,
    localError,
    offer,
    vaults,
    checked,
    waitingForJoin,
    approval,
    outcome,
    copied,
    handoffCopied,
    secondsLeft,
    offerExpired,
    toggleVault,
    beginOffer,
    copyPayload,
    copyHandoff,
    approve,
    reset
  };
}
