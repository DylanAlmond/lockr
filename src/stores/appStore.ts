import { reactive } from 'vue';
import { Account, SafeItem, Vault } from '../types';
import {
  useVault,
  type CreateAccountProps,
  type CreateItemProps,
  type UpdateItemProps
} from '../composables/useVault';

interface AppStore {
  activeAccount: Account | null;
  /** Active item of any kind (notes, and later cards/identities). */
  activeItem: SafeItem | null;
  mutationCount: number;
  vaultMutationCount: number;
}

const state = reactive<AppStore>({
  activeAccount: null,
  activeItem: null,
  mutationCount: 0,
  vaultMutationCount: 0
});

function useAppStore() {
  const {
    getAccountbyId,
    updateAccount,
    deleteAccount,
    addAccount,
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

  async function setActiveAccount(accountId: string | null) {
    if (!accountId) {
      state.activeAccount = null;
      return;
    }
    state.activeAccount = await getAccountbyId(accountId);
  }

  async function updateActiveAccount(data: Partial<Account>): Promise<boolean> {
    if (!state.activeAccount) return false;
    try {
      state.activeAccount = await updateAccount(
        state.activeAccount.vault_id,
        state.activeAccount.id,
        data
      );
      state.mutationCount++;
      return true;
    } catch (error) {
      return false;
    }
  }

  async function createNewAccount(data: CreateAccountProps): Promise<Account | null> {
    try {
      const newAccount = await addAccount(data);
      state.mutationCount++;
      return newAccount;
    } catch (error) {
      return null;
    }
  }

  async function deleteActiveAccount(): Promise<boolean> {
    if (!state.activeAccount) return false;
    try {
      await deleteAccount(state.activeAccount.vault_id, state.activeAccount.id);
      state.activeAccount = null;
      state.mutationCount++;
      return true;
    } catch (error) {
      return false;
    }
  }

  // ============ Items (generic surface) ============

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
    // Keep the account view in sync for account-kind items
    if (item.kind === 'account' && updated.kind === 'account') {
      state.activeAccount = updated;
    }
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
      if (item.kind === 'account') state.activeAccount = null;
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
      if (moved.kind === 'account') state.activeAccount = moved;
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
    setActiveAccount,
    setActiveItem,
    updateActiveAccount,
    createNewAccount,
    deleteActiveAccount,
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
