import { ref } from 'vue';
import { Account, AccountFilter, Vault } from '../types';
import { invoke } from '@tauri-apps/api/core';

const isLoading = ref(false);
const error = ref<string | null>(null);

// Holds the list of vaults currently unlocked in memory
const unlockedVaults = ref<Vault[]>([]);

export function useVault() {
  async function listVaultIds(): Promise<string[]> {
    try {
      return await invoke<string[]>('list_vault_ids');
    } catch (e) {
      error.value = String(e);
      return [];
    }
  }

  async function getUnlockedVaults(): Promise<Vault[]> {
    try {
      unlockedVaults.value = await invoke<Vault[]>('get_unlocked_vaults');
      return unlockedVaults.value;
    } catch (e) {
      error.value = String(e);
      return [];
    }
  }

  async function createVault(name: string): Promise<Vault | null> {
    isLoading.value = true;
    error.value = null;

    try {
      const vault = await invoke<Vault>('create_vault', { name });
      // Automatically add the new vault to local state so the UI updates instantly
      if (vault && !unlockedVaults.value.find((v) => v.id === vault.id)) {
        unlockedVaults.value.push(vault);
      }
      return vault;
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      isLoading.value = false;
    }
  }

  async function updateVault(
    vaultId: string,
    data: { name?: string | null; color?: string | null }
  ): Promise<boolean> {
    try {
      await invoke('update_vault', {
        vaultId,
        name: data.name ?? undefined,
        color: data.color ?? undefined
      });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function deleteVault(vaultId: string): Promise<boolean> {
    try {
      await invoke('delete_vault', { vaultId });
      // Remove from local state
      unlockedVaults.value = unlockedVaults.value.filter((v) => v.id !== vaultId);
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function getAccount(vaultId: string, accountId: string): Promise<Account | null> {
    try {
      return await invoke<Account>('get_account', { vaultId, accountId });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function getAllAccounts(filter?: AccountFilter): Promise<Account[]> {
    try {
      return await invoke<Account[]>('get_all_accounts', { filter: filter || {} });
    } catch (e) {
      error.value = String(e);
      return [];
    }
  }

  async function addAccount(
    vaultId: string,
    username: string,
    password: string,
    displayName?: string | null,
    email?: string | null
  ): Promise<Account | null> {
    try {
      return await invoke<Account>('add_account', {
        vaultId,
        displayName: displayName || null,
        username,
        email: email || null,
        password
      });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function updateAccount(
    vaultId: string,
    accountId: string,
    data: {
      displayName?: string | null;
      username?: string | null;
      email?: string | null;
      password?: string | null;
      favourite?: boolean | null;
      tags?: string[] | null;
      icon?: string | null;
      color?: string | null;
    }
  ): Promise<Account | null> {
    try {
      return await invoke<Account>('update_account', {
        vaultId,
        accountId,
        displayName: data.displayName ?? undefined,
        username: data.username ?? undefined,
        email: data.email ?? undefined,
        password: data.password ?? undefined,
        favourite: data.favourite ?? undefined,
        tags: data.tags ?? undefined,
        icon: data.icon ?? undefined,
        color: data.color ?? undefined
      });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function deleteAccount(vaultId: string, accountId: string): Promise<boolean> {
    try {
      await invoke('delete_account', { vaultId, accountId });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function getSecret(vaultId: string, accountId: string): Promise<string | null> {
    try {
      return await invoke<string>('get_secret', { vaultId, accountId });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  return {
    isLoading,
    error,
    unlockedVaults,

    // Vault Meta
    listVaultIds,
    getUnlockedVaults,

    // Vault Mutations
    createVault,
    updateVault,
    deleteVault,

    // Accounts
    getAccount,
    getAllAccounts,
    addAccount,
    updateAccount,
    deleteAccount,
    getSecret
  };
}
