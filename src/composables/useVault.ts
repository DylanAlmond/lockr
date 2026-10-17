import { ref } from 'vue';
import {
  Entropy,
  ItemFilter,
  NewItemData,
  SafeItem,
  SecretInput,
  Vault
} from '../types';
import { invoke } from '@tauri-apps/api/core';

const isLoading = ref(false);
const error = ref<string | null>(null);

export interface CreateItemProps {
  vaultId: string;
  displayName?: string | null;
  icon?: string | null;
  /** Kind wire payload: internally tagged like the backend's NewItemData */
  data: NewItemData;
  /** Initial secret fields (accounts carry 'password', cards their keys); validated backend-side */
  secrets?: SecretInput[];
}

export interface UpdateItemProps {
  displayName?: string | null;
  icon?: string | null;
  favourite?: boolean | null;
  tags?: string[] | null;
  color?: string | null;
  data?: NewItemData;
}


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

  async function createVault(data: Partial<Omit<Vault, 'id'>>): Promise<Vault | null> {
    isLoading.value = true;
    error.value = null;

    try {
      const vault = await invoke<Vault>('create_vault', { name: data.name, color: data.color });
      return vault;
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      isLoading.value = false;
    }
  }

  async function updateVault(vaultId: string, data: Partial<Omit<Vault, 'id'>>): Promise<boolean> {
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

  // ============ Items (generic surface, all kinds) ============

  async function getItembyId(itemId: string): Promise<SafeItem | null> {
    try {
      return await invoke<SafeItem>('get_item_by_id', { itemId });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function getAllItems(filter?: ItemFilter): Promise<SafeItem[]> {
    try {
      return await invoke<SafeItem[]>('get_all_items', { filter: filter || {} });
    } catch (e) {
      error.value = String(e);
      return [];
    }
  }

  async function addItem(props: CreateItemProps): Promise<SafeItem | null> {
    try {
      return await invoke<SafeItem>('add_item', {
        vaultId: props.vaultId,
        displayName: props.displayName || null,
        icon: props.icon || null,
        data: props.data,
        secrets: props.secrets ?? null
      });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  // ============ Per-field secrets ============

  async function getItemSecret(
    vaultId: string,
    itemId: string,
    fieldKey: string
  ): Promise<string | null> {
    try {
      return await invoke<string>('get_item_secret', { vaultId, itemId, fieldKey });
    } catch (e) {
      error.value = String(e);
      // Errors propagate: null must mean 'no value exists' only, so
      // callers can never mistake a failed fetch for an empty secret
      throw e;
    }
  }

  async function updateItemSecret(
    vaultId: string,
    itemId: string,
    fieldKey: string,
    value: string | null
  ): Promise<boolean> {
    try {
      await invoke('update_item_secret', { vaultId, itemId, fieldKey, value });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function updateItem(
    vaultId: string,
    itemId: string,
    data: UpdateItemProps
  ): Promise<SafeItem | null> {
    try {
      return await invoke<SafeItem>('update_item', {
        vaultId,
        itemId,
        displayName: data.displayName ?? undefined,
        icon: data.icon ?? undefined,
        favourite: data.favourite ?? undefined,
        tags: data.tags ?? undefined,
        color: data.color ?? undefined,
        data: data.data ?? undefined
      });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function deleteItem(vaultId: string, itemId: string): Promise<boolean> {
    try {
      await invoke('delete_item', { vaultId, itemId });
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function moveItem(
    vaultId: string,
    itemId: string,
    targetVaultId: string
  ): Promise<SafeItem | null> {
    try {
      return await invoke<SafeItem>('move_item', { vaultId, itemId, targetVaultId });
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  /**
   * Password strength for the item's `password` field. Returns null only
   * when the item has no stored password (the backend answers "Item has
   * no password" as a validation error for that case).
   */
  async function getItemPasswordStrength(vaultId: string, itemId: string): Promise<Entropy | null> {
    try {
      return await invoke<Entropy>('get_item_password_strength', { vaultId, itemId });
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

    // Items (the generic surface, all kinds)
    getItembyId,
    getAllItems,
    addItem,
    updateItem,
    deleteItem,
    moveItem,

    // Per-field secrets
    getItemSecret,
    updateItemSecret,
    getItemPasswordStrength,

    // Util
    getPasswordStrength,
    flushVault,
    flushAll,
    setAutosave,
    isVaultDirty
  };
}
