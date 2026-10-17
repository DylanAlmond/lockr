import { reactive } from 'vue';
import { SafeItem, Vault } from '../types';
import {
  useVault,
  type CreateItemProps,
  type UpdateItemProps
} from '../composables/useVault';

interface AppStore {
  /** Active item of any kind (account, note, card, identity). */
  activeItem: SafeItem | null;
  mutationCount: number;
  vaultMutationCount: number;
}

const state = reactive<AppStore>({
  activeItem: null,
  mutationCount: 0,
  vaultMutationCount: 0
});

function useAppStore() {
  const {
    createVault,
    updateVault,
    deleteVault,
    getItembyId,
    addItem,
    updateItem,
    deleteItem,
    moveItem
  } = useVault();

  async function setActiveItem(itemId: string | null) {
    if (!itemId) {
      state.activeItem = null;
      return;
    }
    state.activeItem = await getItembyId(itemId);
  }

  // ============ Items (the generic surface, all kinds) ============

  async function createNewItem(props: CreateItemProps): Promise<SafeItem | null> {
    try {
      const newItem = await addItem(props);
      if (newItem) state.mutationCount++;
      return newItem;
    } catch (error) {
      return null;
    }
  }

  async function updateActiveItem(data: UpdateItemProps): Promise<boolean> {
    const item = state.activeItem;
    if (!item) return false;
    const updated = await updateItem(item.vault_id, item.id, data);
    if (!updated) return false;

    state.activeItem = updated;
    state.mutationCount++;
    return true;
  }

  async function deleteActiveItem(): Promise<boolean> {
    const item = state.activeItem;
    if (!item) return false;
    try {
      const success = await deleteItem(item.vault_id, item.id);
      if (!success) return false;

      state.activeItem = null;
      state.mutationCount++;
      return true;
    } catch (error) {
      return false;
    }
  }

  // Generic backend move: one atomic manager mutation (item + secret entry),
  // used for accounts and notes alike; the item keeps its fields and gets a
  // fresh id in the target vault.
  async function moveActiveItem(targetVaultId: string): Promise<SafeItem | null> {
    const item = state.activeItem;
    if (!item || item.vault_id === targetVaultId) return null;

    try {
      const moved = await moveItem(item.vault_id, item.id, targetVaultId);
      if (!moved) return null;

      state.activeItem = moved;
      state.vaultMutationCount++;
      return state.activeItem;
    } catch (error) {
      return null;
    }
  }

  async function createNewVault(data: Partial<Omit<Vault, 'id'>>): Promise<Vault | null> {
    try {
      const vault = await createVault(data);
      if (vault) state.vaultMutationCount++;
      return vault;
    } catch (error) {
      return null;
    }
  }

  async function updateVaultDetails(
    vaultId: string,
    data: Partial<Omit<Vault, 'id'>>
  ): Promise<boolean> {
    try {
      const success = await updateVault(vaultId, data);
      if (success) state.vaultMutationCount++;
      return success;
    } catch (error) {
      return false;
    }
  }

  async function deleteVaultById(vaultId: string): Promise<boolean> {
    try {
      const success = await deleteVault(vaultId);
      if (success) state.vaultMutationCount++;
      return success;
    } catch (error) {
      return false;
    }
  }

  return {
    state,
    setActiveItem,
    createNewItem,
    updateActiveItem,
    deleteActiveItem,
    moveActiveItem,
    createNewVault,
    updateVaultDetails,
    deleteVaultById
  };
}

export default useAppStore;
