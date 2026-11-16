import { ref } from 'vue';
import { useSync } from './useSync';
import type { DiscoveredPeer } from '../types';

/**
 * The Devices screen's one discovery session. Discovery is user-screen-scoped:
 * entering starts it, leaving stops it, and polling only refreshes the list.
 */
export function useDiscovery() {
  const {
    error,
    isSyncing,
    startDiscovery,
    stopDiscovery,
    listDiscoveredPeers,
    startServing,
    stopServing
  } = useSync();

  const peers = ref<DiscoveredPeer[]>([]);
  const discoveryError = ref<string | null>(null);
  const discoveryOn = ref(false);
  const loadingPeers = ref(false);

  let pollTimer: ReturnType<typeof setInterval> | null = null;

  async function refreshPeers() {
    if (isSyncing.value) return;

    loadingPeers.value = true;
    const listed = await listDiscoveredPeers();
    loadingPeers.value = false;

    if (listed) {
      peers.value = listed;
      discoveryError.value = null;
    } else {
      discoveryError.value = error.value ?? 'Devices could not be discovered.';
    }
  }

  async function beginDiscovery() {
    // A previous screen's session may still be tearing down. Starting from a
    // known-stopped state keeps a quick navigate-away-and-back deterministic.
    await stopDiscovery();

    const started = await startDiscovery();
    discoveryOn.value = started;

    if (!started) {
      discoveryError.value = error.value ?? 'Devices could not be discovered.';
      return;
    }

    // Serving needs an open discovery session to have a listener; starting it
    // never advertises on its own.
    await startServing();

    await refreshPeers();
  }

  function startPolling(intervalMs = 3000) {
    stopPolling();
    pollTimer = setInterval(() => {
      void refreshPeers();
    }, intervalMs);
  }

  function stopPolling() {
    if (pollTimer) clearInterval(pollTimer);
    pollTimer = null;
  }

  /** Leave the network exactly as it was found. */
  async function finishDiscovery() {
    stopPolling();
    if (!discoveryOn.value) return;

    await stopServing();
    await stopDiscovery();
    discoveryOn.value = false;
  }

  return {
    peers,
    discoveryError,
    discoveryOn,
    loadingPeers,
    refreshPeers,
    beginDiscovery,
    startPolling,
    stopPolling,
    finishDiscovery
  };
}
