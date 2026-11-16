import { request, requestOk } from './syncState';
import type { SyncSetView, UnregisteredVault } from '../types';

/** The Device's Sync Set and local Vault adoption. */
export function useSyncSet() {
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

  return {
    getSyncSet,
    setSyncSet,
    setVaultSyncing,
    listUnregisteredVaults,
    adoptVault
  };
}
