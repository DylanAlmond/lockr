import { ref } from 'vue';
import { Vault } from '../types';
import { invoke } from '@tauri-apps/api/core';

const currentVault = ref<Vault | null>(null);
const isLoading = ref(false);
const error = ref<string | null>(null);

export function useVault() {
  async function createVault(name: string, masterPassword: string): Promise<Vault | null> {
    isLoading.value = true;
    error.value = null;

    try {
      const vault = await invoke<Vault>('create_vault', { name, masterPassword });

      currentVault.value = vault;
      return vault;
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      isLoading.value = false;
    }
  }

  async function listVaultIds(): Promise<string[]> {
    isLoading.value = true;
    error.value = null;

    try {
      // invoke returns a promise. We specify it returns an array of strings
      return await invoke<string[]>('list_vault_ids');
    } catch (e) {
      error.value = String(e);
      return [];
    } finally {
      isLoading.value = false;
    }
  }

  async function unlockVault(vaultId: string, masterPassword: string): Promise<Vault | null> {
    isLoading.value = true;
    error.value = null;

    try {
      const vault = await invoke<Vault>('unlock_vault', {
        vaultId,
        masterPassword
      });

      currentVault.value = vault;
      return vault;
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      isLoading.value = false;
    }
  }

  return {
    currentVault,
    isLoading,
    error,
    createVault,
    listVaultIds,
    unlockVault
  };
}
