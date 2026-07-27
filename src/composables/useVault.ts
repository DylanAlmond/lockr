import { ref } from 'vue';
import { Account, AccountFilter, Entropy, Vault } from '../types';
import { invoke } from '@tauri-apps/api/core';

const isLoading = ref(false);
const error = ref<string | null>(null);

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
      return await invoke<Vault[]>('get_unlocked_vaults');
    } catch (e) {
      error.value = String(e);
      return [];
    }
  }

  async function getVaultById(vaultId: string): Promise<Vault | null> {
    try {
      return await invoke<Vault>('get_vault_by_id', { vaultId });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function createVault(name: string): Promise<Vault | null> {
    isLoading.value = true;
    error.value = null;

    try {
      const vault = await invoke<Vault>('create_vault', { name });
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
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function getAccountbyId(accountId: string): Promise<Account | null> {
    try {
      return await invoke<Account>('get_account_by_id', { accountId });
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

  async function getAccountPasswordStrength(
    vaultId: string,
    accountId: string
  ): Promise<Entropy | null> {
    try {
      return await invoke<Entropy>('get_account_password_strength', { vaultId, accountId });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function getPasswordStrength(password: string): Promise<Entropy | null> {
    try {
      return await invoke<Entropy>('get_password_strength', { password });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function flushVault(vaultId: string): Promise<boolean> {
    try {
      await invoke<boolean>('flush_vault', { vaultId });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function flushAll(): Promise<boolean> {
    try {
      await invoke<boolean>('flush_all');
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function setAutosave(enabled: boolean): Promise<boolean> {
    try {
      await invoke<boolean>('set_autosave', { enabled });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function isVaultDirty(vaultId: string): Promise<boolean> {
    try {
      return await invoke<boolean>('is_vault_dirty', { vaultId });
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  return {
    isLoading,
    error,

    // Vault Meta
    listVaultIds,
    getUnlockedVaults,
    getVaultById,

    // Vault Mutations
    createVault,
    updateVault,
    deleteVault,

    // Accounts
    getAccountbyId,
    getAllAccounts,
    addAccount,
    updateAccount,
    deleteAccount,
    getSecret,
    getAccountPasswordStrength,

    // Util
    getPasswordStrength,
    flushVault,
    flushAll,
    setAutosave,
    isVaultDirty
  };
}
