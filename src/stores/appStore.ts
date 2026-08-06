import { reactive } from 'vue';
import { Account, Vault } from '../types';
import { useVault } from '../composables/useVault';

interface AppStore {
  editMode: boolean;
  activeAccount: Account | null;
  activeVault: Vault | null;
  isLoadingActive: boolean;
}

const state = reactive<AppStore>({
  editMode: false,
  activeAccount: null,
  activeVault: null,
  isLoadingActive: false
});

function useAppStore() {
  const {
    getAccountbyId,
    getVaultById,
    updateAccount,
    updateAccountPassword
  } = useVault();

  function setEditMode(value: boolean) {
    state.editMode = value;
  }

  // Called when the route param `passwordId` changes
  async function setActiveAccount(accountId: string | null) {
    if (!accountId) {
      state.activeAccount = null;
      state.activeVault = null;
      return;
    }

    state.isLoadingActive = true;
    try {
      const account = await getAccountbyId(accountId);

      if (account) {
        const [vault] = await Promise.all([
          getVaultById(account.vault_id),
        ]);
        state.activeAccount = account;
        state.activeVault = vault;
      } else {
        state.activeAccount = null;
        state.activeVault = null;
      }
    } finally {
      state.isLoadingActive = false;
    }
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
      console.log(error);

      return false;
    }
  }

  return {
    state,
    setEditMode,
    setActiveAccount,
    updateActiveAccount,
    updateActiveAccountPassword
  };
}

export default useAppStore;
