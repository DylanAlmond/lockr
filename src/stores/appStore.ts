import { reactive } from 'vue';
import { Account, Vault } from '../types';
import { CreateAccountProps, useVault } from '../composables/useVault';

interface AppStore {
  activeAccount: Account | null;
  mutationCount: number;
  vaultMutationCount: number;
}

const state = reactive<AppStore>({
  activeAccount: null,
  mutationCount: 0,
  vaultMutationCount: 0
});

function useAppStore() {
  const {
    getAccountbyId,
    updateAccount,
    updateAccountPassword,
    deleteAccount,
    addAccount,
    createVault,
    updateVault,
    deleteVault
  } = useVault();

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

  async function updateActiveAccountPassword(password: string | null): Promise<boolean> {
    if (!state.activeAccount) return false;
    try {
      state.activeAccount = await updateAccountPassword(
        state.activeAccount.vault_id,
        state.activeAccount.id,
        password
      );
      state.mutationCount++;
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
      state.mutationCount++;
      return true;
    } catch (error) {
      return false;
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
    updateActiveAccount,
    createNewAccount,
    updateActiveAccountPassword,
    deleteActiveAccount,
    createNewVault,
    updateVaultDetails,
    deleteVaultById
  };
}

export default useAppStore;
