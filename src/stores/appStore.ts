import { reactive } from 'vue';
import { Account } from '../types';
import { useVault } from '../composables/useVault';

interface AppStore {
  editMode: boolean;
  activeAccount: Account | null;
}

const state = reactive<AppStore>({
  editMode: false,
  activeAccount: null
});

function useAppStore() {
  const { getAccountbyId, updateAccount, updateAccountPassword, deleteAccount } = useVault();

  function setEditMode(value: boolean) {
    state.editMode = value;
  }

  // Called when the route param `passwordId` changes
  async function setActiveAccount(accountId: string | null) {
    if (!accountId) {
      state.activeAccount = null;
      return;
    }

    state.activeAccount = await getAccountbyId(accountId);
  }

  // Helpers to update the local store immediately after an API mutation
  async function updateActiveAccount(data: Partial<Account>): Promise<boolean> {
    if (!state.activeAccount) return false;

    try {
      state.activeAccount = await updateAccount(
        state.activeAccount.vault_id,
        state.activeAccount.id,
        data
      );

      return true;
    } catch (error) {
      return false;
    }
  }

  async function updateActiveAccountPassword(password: string | null): Promise<boolean> {
    if (!state.activeAccount) return false;

    try {
      state.activeAccount = await updateAccountPassword(
        state.activeAccount.vault_id,
        state.activeAccount.id,
        password
      );

      return true;
    } catch (error) {
      return false;
    }
  }

  async function deleteActiveAccount(): Promise<boolean> {
    if (!state.activeAccount) return false;

    try {
      await deleteAccount(state.activeAccount.vault_id, state.activeAccount.id);
      state.activeAccount = null;

      return true;
    } catch (error) {
      return false;
    }
  }

  return {
    state,
    setEditMode,
    setActiveAccount,
    updateActiveAccount,
    updateActiveAccountPassword,
    deleteActiveAccount
  };
}

export default useAppStore;
