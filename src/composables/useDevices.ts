import { computed, ref } from 'vue';
import { useSync } from './useSync';
import type { PairedDevice, SyncSetView, UnregisteredVault } from '../types';
import {
  deviceNamesFrom,
  latestLastSyncOf,
  pairedPeersOf,
  thisDeviceOf,
  vaultNamesFrom
} from '../util/devices';

/**
 * Device-list, Sync Set and unregistered-file state for the Devices screen.
 * The discovery session and the sync run are separate composables; this one
 * only reads what the backend already knows.
 */
export function useDevices() {
  const { listPairedDevices, getSyncSet, listUnregisteredVaults } = useSync();

  const devices = ref<PairedDevice[]>([]);
  const syncSet = ref<SyncSetView | null>(null);
  const unregistered = ref<UnregisteredVault[]>([]);
  const loadingDevices = ref(false);

  const vaultNames = computed(() => vaultNamesFrom(syncSet.value));
  const deviceNames = computed(() => deviceNamesFrom(devices.value));
  const thisDevice = computed(() => thisDeviceOf(devices.value));
  const pairedPeers = computed(() => pairedPeersOf(devices.value));
  const syncSetVaults = computed(() => syncSet.value?.vaults ?? []);
  const latestLastSync = computed(() => latestLastSyncOf(devices.value));

  async function refreshDevices() {
    loadingDevices.value = true;
    const listed = await listPairedDevices();
    if (listed) devices.value = listed;
    loadingDevices.value = false;
  }

  async function refreshSyncSet() {
    const set = await getSyncSet();
    if (set) syncSet.value = set;
  }

  async function refreshUnregistered() {
    const listed = await listUnregisteredVaults();
    if (listed) unregistered.value = listed;
  }

  async function refreshAll() {
    await Promise.all([refreshDevices(), refreshSyncSet(), refreshUnregistered()]);
  }

  /** The data a clean sync or a closed Conflict can change. */
  async function refreshAfterSync() {
    await Promise.all([refreshSyncSet(), refreshUnregistered()]);
  }

  return {
    devices,
    syncSet,
    unregistered,
    loadingDevices,
    vaultNames,
    deviceNames,
    thisDevice,
    pairedPeers,
    syncSetVaults,
    latestLastSync,
    refreshDevices,
    refreshSyncSet,
    refreshUnregistered,
    refreshAll,
    refreshAfterSync
  };
}
