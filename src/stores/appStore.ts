import { reactive } from 'vue';
import { Account, Vault } from '../types';
import { useVault, type CreateAccountProps } from '../composables/useVault';

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
    getSecret,
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

  // No backend "move" primitive exists — recreate the account in the target
  // vault (preserving its fields/secret) then remove it from the source vault.
  async function moveActiveAccount(targetVaultId: string): Promise<Account | null> {
    const account = state.activeAccount;
    if (!account || account.vault_id === targetVaultId) return null;

    try {
      const password = (await getSecret(account.vault_id, account.id)) || '';

      const newAccount = await addAccount({
        vaultId: targetVaultId,
        username: account.username,
        password,
        displayName: account.display_name,
        email: account.email,
        icon: account.icon
      });
      if (!newAccount) return null;

      const finalAccount = await updateAccount(targetVaultId, newAccount.id, {
        favourite: account.favourite,
        tags: account.tags,
        color: account.color
      });

      await deleteAccount(account.vault_id, account.id);

      state.activeAccount = finalAccount || newAccount;
      state.mutationCount++;
      return state.activeAccount;
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
    updateActiveAccount,
    createNewAccount,
    updateActiveAccountPassword,
    deleteActiveAccount,
    moveActiveAccount,
    createNewVault,
    updateVaultDetails,
    deleteVaultById
  };
}

export default useAppStore;
