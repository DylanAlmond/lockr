import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { User, Vault } from '../types';
import { useVault } from './useVault';

const { getUnlockedVaults } = useVault();

const user = ref<User | null>(null);
const isLoading = ref(false);
const error = ref<string | null>(null);

export function useUser() {
  async function register(name: string, masterPassword: string): Promise<Vault[] | null> {
    isLoading.value = true;
    error.value = null;

    try {
      const vaults = await invoke<Vault[]>('register_user', { name, masterPassword });
      // Backend handles the login, but we still need to grab the user profile data (name, color)
      await fetchUser();
      await getUnlockedVaults();

      return vaults;
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      isLoading.value = false;
    }
  }

  async function login(masterPassword: string): Promise<Vault[] | null> {
    isLoading.value = true;
    error.value = null;

    try {
      const vaults = await invoke<Vault[]>('login_user', { masterPassword });

      await fetchUser();
      await getUnlockedVaults();

      return vaults;
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      isLoading.value = false;
    }
  }

  async function logout(): Promise<boolean> {
    try {
      await invoke('logout');
      user.value = null;
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  async function fetchUser(): Promise<User | null> {
    try {
      user.value = await invoke<User>('get_user');
      return user.value;
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  async function updateProfile(data: {
    name?: string | null;
    color?: string | null;
    icon?: string | null;
  }): Promise<boolean> {
    isLoading.value = true;
    error.value = null;

    try {
      await invoke('update_profile', {
        name: data.name ?? undefined,
        color: data.color ?? undefined,
        icon: data.icon ?? undefined
      });

      // Optimistic local update
      if (user.value) {
        if (data.name) user.value.name = data.name;
        if (data.color) user.value.color = data.color;
        if (data.icon) user.value.icon = data.icon;
      }

      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    } finally {
      isLoading.value = false;
    }
  }

  async function deleteUser(): Promise<Boolean> {
    try {
      await invoke<User>('delete_user');
      user.value = null;
      return true;
    } catch (e) {
      error.value = String(e);
      return false;
    }
  }

  return {
    user,
    isLoading,
    error,
    register,
    login,
    logout,
    fetchUser,
    updateProfile,
    deleteUser
  };
}
