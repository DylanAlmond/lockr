<script setup lang="ts">
import { useRoute } from 'vue-router';
import { useVault } from '../../composables/useVault';
import { Account, Entropy, Vault } from '../../types';
import { ref, watch } from 'vue';
import { ChevronRight, EllipsisVertical, Eye, EyeOff, Lock, Pencil, Star } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';

const PASSWORDSTRENGTHS = ['Very Weak', 'Weak', 'Fair', 'Good', 'Excellent'];

const route = useRoute();
const { getAccountbyId, getVaultById, getSecret, getAccountPasswordStrength } = useVault();

const vault = ref<Vault | null>(null);
const account = ref<Account | null>(null);
const loading = ref(false);

const passwordEntropy = ref<Entropy | null>();

const password = ref<string | null>(null);
const showPassword = ref(false);

async function togglePassword() {
  console.log(showPassword.value);

  if (!password.value && showPassword.value) {
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

watch(
  () => route.params.passwordId,
  async (id) => {
    if (!id) {
      account.value = null;
      return;
    }

    loading.value = true;

    try {
      account.value = await getAccountbyId(id as string);
      vault.value = await getVaultById(account.value!.vault_id);

      passwordEntropy.value = await getAccountPasswordStrength(
        account.value!.vault_id,
        id as string
      );
    } finally {
      loading.value = false;
    }
  },
  { immediate: true }
);
</script>

<template>
  <div v-if="!account" class="wrapper">No account found.</div>

  <div v-else class="wrapper">
    <header>
      <div class="vault-label">
        <Lock :size="20" aria-hidden="true" :color="vault?.color || '#6240BF'" />
        <span>{{ vault?.name || 'Unknown Vault' }}</span>
      </div>

      <nav class="header-toolbar">
        <Button
          aria-label="Favourite"
          icon-only
          variant="outline"
          size="small"
          :icon-component="Star"
        />

        <Button variant="outline" size="small" :icon-component="Pencil">Edit</Button>

        <Button
          class="menu-button"
          aria-label="Account Menu"
          icon-only
          variant="label"
          size="small"
          :icon-component="EllipsisVertical"
        />
      </nav>
    </header>

    <main class="thin-scrollbar">
      <section class="descriptor-section">
        <div class="account-icon">
          {{ (account.display_name || account.username)[0].toUpperCase() }}
        </div>
        <h1 class="display-name">{{ account.display_name }}</h1>
      </section>

      <section class="account-fields-section">
        <!-- Username -->
        <AccountField
          label="username"
          :display-value="account.username"
          :copy-value="account.username"
        />

        <!-- Email -->
        <AccountField
          label="email"
          :display-value="account.email || 'No Email'"
          :copy-value="account.email"
        />

        <!-- Password -->
        <AccountField
          label="password"
          :display-value="showPassword ? password || '••••••••••••••••' : '••••••••••••••••'"
          :copy-value="fetchPassword"
        >
          <template #actions>
            <span v-if="passwordEntropy?.score" class="password-strength">
              {{ PASSWORDSTRENGTHS[passwordEntropy.score] }}
            </span>

            <Button
              aria-label="View Password"
              icon-only
              variant="outline"
              size="small"
              :icon-component="showPassword ? EyeOff : Eye"
              @click="togglePassword"
            />
          </template>
        </AccountField>
      </section>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList :tags="account.tags" />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(account.updated_at)} ` }}</span>
      </section>
    </main>
  </div>
</template>

<style scoped>
.wrapper {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1rem;
}

main {
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  gap: 1.5rem;
  padding: 1rem;
  flex: 1;

  overflow-y: auto;
  overflow-x: hidden;
}

.menu-button {
  --button-icon-size: 1.5rem;
}

.vault-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;

  > span {
    font-size: 1.125rem;
    color: var(--color-text-tertiary);
    font-weight: 350;

    text-box-trim: trim-both;
    text-box-edge: cap alphabetic;
  }
}

.header-toolbar {
  display: flex;
  gap: 0.75rem;
}

.descriptor-section {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 0.75rem 0rem;
}

.account-icon {
  display: flex;
  justify-content: center;
  align-items: center;
  width: 5.25rem;
  height: 5.25rem;
  aspect-ratio: 1/1;

  font-size: 2rem;
  font-family: var(--font-geo);
  font-weight: 500;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);

  border-radius: 0.75rem;
  box-shadow: var(--shadow-sm);
}

.display-name {
  font-size: 2rem;
  font-family: var(--font-geo);
}

.account-fields-section {
  display: flex;
  flex-direction: column;
  width: 100%;

  border: 1px solid var(--color-border);
  border-radius: 0.75rem;

  & > *:not(:last-child) {
    border-bottom: 1px solid var(--color-border);
  }
}

.password-strength {
  margin-right: 0.5rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-green);
}

.tags-section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0rem 1.5rem;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }
}

.timestamp-section {
  display: flex;
  align-items: center;
  gap: 0.75rem;

  padding: 1.5rem;
  padding-right: 0rem;

  text-box-trim: trim-both;
  text-box-edge: cap alphabetic;

  > svg {
    color: var(--color-text-muted);
  }
}
</style>
