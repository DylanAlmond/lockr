import { ref } from 'vue';
import { Account, Service, Vault } from '../types';
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

  async function updateVaultName(newName: string) {
    try {
      await invoke('update_vault_name', { newName });
      if (currentVault.value) currentVault.value.name = newName;
    } catch (e) {
      error.value = String(e);
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

  async function addService(name: string) {
    try {
      const newService = await invoke<Service>('add_service', { name });
      // Find the service in our local state and add it so Vue updates instantly
      currentVault.value?.services.push(newService);
      return newService;
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function updateServiceName(serviceId: string, newName: string) {
    try {
      await invoke('update_service_name', { serviceId, newName });
      const service = currentVault.value?.services.find((s) => s.id === serviceId);
      if (service) service.name = newName;
    } catch (e) {
      error.value = String(e);
    }
  }

  async function deleteService(serviceId: string) {
    try {
      await invoke('delete_service', { serviceId });
      // Remove from local state
      if (currentVault.value) {
        currentVault.value.services = currentVault.value.services.filter((s) => s.id !== serviceId);
      }
    } catch (e) {
      error.value = String(e);
    }
  }

  async function addAccount(
    serviceId: string,
    username: string,
    password: string,
    displayName?: string | null,
    email?: string | null
  ) {
    try {
      const newAccount = await invoke<Account>('add_account', {
        serviceId,
        displayName: displayName || null,
        username,
        email: email || null,
        password
      });
      const service = currentVault.value?.services.find((s) => s.id === serviceId);
      service?.accounts.push(newAccount);
      return newAccount;
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function updateAccount(
    serviceId: string,
    accountId: string,
    data: {
      displayName?: string | null;
      username?: string;
      email?: string | null;
      password?: string;
    }
  ) {
    try {
      const updated = await invoke<Account>('update_account', {
        serviceId,
        accountId,
        displayName: data.displayName ?? undefined, // undefined means "don't send to Rust"
        username: data.username ?? undefined,
        email: data.email ?? undefined,
        password: data.password ?? undefined
      });

      // Update local state
      const service = currentVault.value?.services.find((s) => s.id === serviceId);
      const accountIndex = service?.accounts.findIndex((a) => a.id === accountId) ?? -1;
      if (service && accountIndex >= 0) {
        service.accounts[accountIndex] = updated;
      }
      return updated;
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function deleteAccount(serviceId: string, accountId: string) {
    try {
      await invoke('delete_account', { serviceId, accountId });
      // Remove from local state
      const service = currentVault.value?.services.find((s) => s.id === serviceId);
      if (service) {
        service.accounts = service.accounts.filter((a) => a.id !== accountId);
      }
    } catch (e) {
      error.value = String(e);
    }
  }

  return {
    currentVault,
    isLoading,
    error,
    createVault,
    listVaultIds,
    unlockVault,
    addService,
    deleteService,
    addAccount,
    deleteAccount,
    lockVault,
    updateAccount,
    updateServiceName,
    updateVaultName
  };
}
