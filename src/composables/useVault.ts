import { ref } from 'vue';
import { Account, AccountFilter, Vault } from '../types';
import { invoke } from '@tauri-apps/api/core';

const currentVault = ref<Vault | null>(null);
const isLoading = ref(false);
const error = ref<string | null>(null);

export function useVault() {
  async function isVaultUnlocked(vaultId: string): Promise<boolean> {
    try {
      return await invoke<boolean>('is_vault_unlocked', { vaultId });
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function isAnyUnlocked(): Promise<boolean> {
    try {
      return await invoke<boolean>('is_any_unlocked');
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

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
      return await invoke<Vault[]>('get_unlocked_vaults');
    } catch (e) {
      error.value = String(e);
      return [];
    }
  }

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

  async function lockVault() {
    try {
      await invoke('lock_vault');
      currentVault.value = null; // Clear frontend state
    } catch (e) {
      error.value = String(e);
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
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
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
      // If no filter is provided, pass an empty object so Rust uses defaults
      return await invoke<Account[]>('get_all_accounts', { filter: filter || {} });
    } catch (e) {
      error.value = String(e);
      return [];
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

  async function getSecret(serviceId: string, accountId: string): Promise<string | null> {
    try {
      return await invoke<string>('get_secret', { serviceId, accountId });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  return {
    isLoading,
    error,

    // Vault Meta
    isVaultUnlocked,
    isAnyUnlocked,
    listVaultIds,
    getUnlockedVaults,

    // Vault Mutations
    createVault,
    unlockVault,
    lockVault,
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
