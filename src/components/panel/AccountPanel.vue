<script setup lang="ts">
import { useRoute } from 'vue-router';
import { useVault } from '../../composables/useVault';
import { Account, Entropy, Vault } from '../../types';
import { ref, watch } from 'vue';
import useAppStore from '../../stores/appStore.ts';
import AccountDetails from './AccountDetails.vue';
import AccountEdit from './AccountEdit.vue';

const route = useRoute();
const { getAccountbyId, getVaultById, getSecret, getAccountPasswordStrength, updateAccount } =
  useVault();

const { state, setEditMode } = useAppStore();

const vault = ref<Vault | null>(null);
const account = ref<Account | null>(null);
const loading = ref(false);

const passwordEntropy = ref<Entropy | null>(null);
const password = ref<string | null>(null);
const showPassword = ref(false);

async function togglePassword() {
  // If we are about to show the password and don't have it yet, fetch it
  if (!password.value && !showPassword.value) {
    await fetchPassword();
  }
  showPassword.value = !showPassword.value;
}

async function fetchPassword() {
  if (!account.value) return null;

  if (!password.value) {
    password.value = await getSecret(account.value.vault_id, account.value.id);
  }

  return password.value;
}

async function toggleFavourite() {
  if (!vault.value || !account.value) return;

  account.value = await updateAccount(vault.value!.id, account.value!.id, {
    favourite: !account.value?.favourite
  });
}

async function handleSave(data: Partial<Account>) {
  if (!vault.value || !account.value) return;

  account.value = await updateAccount(vault.value!.id, account.value!.id, { ...data });

  console.log(account.value);

  setEditMode(false);
}

watch(
  () => route.params.passwordId,
  async (id) => {
    if (!id) {
      account.value = null;
      vault.value = null;
      passwordEntropy.value = null;
      password.value = null;
      showPassword.value = false;
      return;
    }

    loading.value = true;

    try {
      const nextAccount = await getAccountbyId(id as string);

      let nextVault: Vault | null = null;
      let nextEntropy: Entropy | null = null;

      if (nextAccount) {
        [nextVault, nextEntropy] = await Promise.all([
          getVaultById(nextAccount.vault_id),
          getAccountPasswordStrength(nextAccount.vault_id, nextAccount.id)
        ]);
      }

      password.value = null;
      showPassword.value = false;

      account.value = nextAccount;
      vault.value = nextVault;
      passwordEntropy.value = nextEntropy;
    } finally {
      loading.value = false;
    }
  },
  { immediate: true }
);
</script>

<template>
  <AccountEdit
    v-if="state.editMode"
    :account="account"
    :vault="vault"
    @cancel="setEditMode(false)"
    @save="handleSave"
  />
  <AccountDetails
    v-else
    :account="account"
    :vault="vault"
    :password-entropy="passwordEntropy"
    :password="password"
    :show-password="showPassword"
    :fetch-password="fetchPassword"
    @toggle-password="togglePassword"
    @toggle-favourite="toggleFavourite"
    @edit="setEditMode(true)"
  />
</template>
