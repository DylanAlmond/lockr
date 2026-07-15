import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { User } from '../types';

const user = ref<User | null>(null);
const isLoading = ref(false);
const error = ref<string | null>(null);

export function useUser() {
  async function fetchUser(): Promise<User | null> {
    isLoading.value = true;
    error.value = null;

    try {
      user.value = await invoke<User>('get_user');
      return user.value;
    } catch (e) {
      error.value = String(e);
      return null;
    } finally {
      isLoading.value = false;
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

  return {
    user,
    isLoading,
    error,
    fetchUser,
    updateProfile
  };
}
