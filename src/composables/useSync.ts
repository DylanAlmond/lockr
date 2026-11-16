import {
  clearSyncFailure,
  conflictError,
  conflicts,
  moveResidueNotes,
  error,
  isSyncing,
  lastReport,
  progress,
  resetSyncState,
  syncFailure,
  syncingPeerId
} from './syncState';
import { useSyncSession } from './useSyncSession';
import { useSyncPairing } from './useSyncPairing';
import { useSyncDevices } from './useSyncDevices';
import { useSyncSet } from './useSyncSet';

export { classifySyncError } from '../util/syncErrors';
export type { SyncFailure, SyncFailureKind } from '../util/syncErrors';
export type {
  ConflictResolutionRequest,
  ConflictSession,
  RevealedConflictValue
} from './syncState';

/**
 * The pairing, discovery, sync, Sync Set and Device-identity commands.
 *
 * Split by concern so no devices-area file carries the whole surface. Sync
 * progress and the open Conflict session stay module-level: a sync can
 * outlive the screen that started it.
 */
export function useSync() {
  const session = useSyncSession();
  const pairing = useSyncPairing();
  const devices = useSyncDevices();
  const syncSet = useSyncSet();

  function resetSyncSession(): void {
    resetSyncState();
  }

  return {
    error,
    isSyncing,
    syncingPeerId,
    progress,
    syncFailure,
    conflicts,
    moveResidueNotes,
    lastReport,
    conflictError,
    isResolvingConflict: session.isResolvingConflict,

    clearSyncFailure,
    resetSyncSession,

    // Pairing
    startPairing: pairing.startPairing,
    cancelPairing: pairing.cancelPairing,
    joinPairing: pairing.joinPairing,
    receivePairingRequest: pairing.receivePairingRequest,
    approvePairing: pairing.approvePairing,
    completePairing: pairing.completePairing,

    // Devices and discovery
    listPairedDevices: devices.listPairedDevices,
    startDiscovery: devices.startDiscovery,
    stopDiscovery: devices.stopDiscovery,
    listDiscoveredPeers: devices.listDiscoveredPeers,
    revokeDevice: devices.revokeDevice,
    startServing: pairing.startServing,
    stopServing: pairing.stopServing,

    // Sync
    syncWithDevice: session.syncWithDevice,
    cancelSync: session.cancelSync,
    onSyncProgress: session.onSyncProgress,
    resolveSyncConflict: session.resolveSyncConflict,
    revealConflictCandidate: session.revealConflictCandidate,
    abandonConflicts: session.abandonConflicts,

    // Sync Set
    getSyncSet: syncSet.getSyncSet,
    setSyncSet: syncSet.setSyncSet,
    setVaultSyncing: syncSet.setVaultSyncing,
    listUnregisteredVaults: syncSet.listUnregisteredVaults,
    adoptVault: syncSet.adoptVault,

    // Device identity
    getDeviceIdentityError: devices.getDeviceIdentityError,
    startWithNewDeviceIdentity: devices.startWithNewDeviceIdentity
  };
}
