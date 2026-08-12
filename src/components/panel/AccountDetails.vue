<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight, EllipsisVertical, Eye, EyeOff, Lock, Pencil, Star } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import { PASSWORDSTRENGTHS } from '../../util/constants.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import AlertModal from '../ui/AlertModal.vue';
import { useModal } from '../../composables/useModal.ts';
import { useVault } from '../../composables/useVault.ts';
import { markRaw } from 'vue';
import ChangePasswordModal from '../ui/ChangePasswordModal.vue';
import useAppStore from '../../stores/appStore.ts';
import { Entropy, Vault } from '../../types/index.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveAccount, deleteActiveAccount } = useAppStore();
const { getSecret, getAccountPasswordStrength, getVaultById } = useVault();
const { openModal } = useModal();

const password = ref<string | null>(null);
const passwordEntropy = ref<Entropy | null>(null);
const showPassword = ref(false);

const parentVault = ref<Vault | null>(null);

async function fetchPassword() {
  if (!state.activeAccount) return null;
  if (!password.value) {
    password.value = await getSecret(state.activeAccount.vault_id, state.activeAccount.id);
  }
  return password.value;
}

async function togglePassword() {
  if (!showPassword.value && !password.value) {
    await fetchPassword();
  }
  showPassword.value = !showPassword.value;
}

async function toggleFavourite() {
  if (!state.activeAccount) return;
  const newFav = !state.activeAccount.favourite;
  updateActiveAccount({ favourite: newFav });
}

function goToEdit() {
  if (!state.activeAccount) return;
  router.push({
    name: route.name as string,
    params: {
      ...route.params,
      accountId: state.activeAccount.id,
      mode: 'edit'
    },
    query: route.query
  });
}

async function handleDelete() {
  if (!state.activeAccount) return;

  const success = await deleteActiveAccount();

  if (success) {
    await router.replace({
      name: route.name as string,
      params: {
        ...route.params,
        accountId: undefined, // Clear selection
        mode: undefined // Clear any modes
      },
      query: route.query
    });
  }
}

const miscMenuItems = computed<DropdownItem[]>(() => [
  {
    label: 'Change Password',
    disabled: state.activeAccount == null,
    onSelect: () => {
      if (!state.activeAccount) return;
      openModal(markRaw(ChangePasswordModal), {
        onClose: () => {},
        onConfirm: () => {}
      });
    }
  },
  {
    label: 'Delete Account',
    onSelect: () => {
      openModal(markRaw(AlertModal), {
        title:
          'Delete ' +
          (state.activeAccount?.display_name ? `"${state.activeAccount.display_name}"` : 'Account'),
        message: "Are you sure you want to continue? This can't be undone.",
        actionLabel: 'Delete',
        onClose: () => {},
        onConfirm: () => handleDelete()
      });
    }
  }
]);

// clear the local password state so it fetches the correct one next time.
watch(
  () => [state.activeAccount?.id, state.activeAccount?.updated_at],
  async () => {
    password.value = null;
    showPassword.value = false;

    if (state.activeAccount) {
      const [vault, entropy] = await Promise.all([
        getVaultById(state.activeAccount.vault_id),
        getAccountPasswordStrength(state.activeAccount?.vault_id, state.activeAccount?.id)
      ]);

      parentVault.value = vault;
      passwordEntropy.value = entropy;
    } else {
      parentVault.value = null;
      passwordEntropy.value = null;
    }
  },
  { immediate: true }
);
</script>

<template>
  <!-- Empty -->
  <div v-if="!state.activeAccount" class="wrapper">No account found.</div>

  <!-- View Mode -->
  <div v-else class="wrapper">
    <header>
      <div class="vault-label">
        <Lock :size="20" aria-hidden="true" :color="parentVault?.color || '#6240BF'" />
        <span>{{ parentVault?.name || 'Unknown Vault' }}</span>
      </div>

      <nav class="header-toolbar">
        <Button
          aria-label="Favourite"
          icon-only
          variant="outline"
          size="small"
          :icon-component="Star"
          :icon-props="{
            fill: state.activeAccount.favourite ? 'var(--color-accent)' : 'none',
            color: state.activeAccount.favourite ? 'var(--color-accent)' : undefined
          }"
          @click="toggleFavourite"
        />

        <Button variant="outline" size="small" :icon-component="Pencil" @click="goToEdit">
          Edit
        </Button>

        <Dropdown :list="miscMenuItems" #trigger="{ triggerProps }">
          <Button
            class="menu-button"
            aria-label="Account Menu"
            icon-only
            variant="label"
            size="small"
            :icon-component="EllipsisVertical"
            v-bind="triggerProps"
          />
        </Dropdown>
      </nav>
    </header>

    <main class="thin-scrollbar">
      <section class="descriptor-section">
        <div class="account-icon">
          {{ (state.activeAccount.display_name || state.activeAccount.username)[0].toUpperCase() }}
          <Star
            v-if="state.activeAccount.favourite"
            :size="32"
            :fill="state.activeAccount.favourite ? 'var(--color-accent)' : undefined"
          />
        </div>
        <h1 class="display-name">{{ state.activeAccount.display_name }}</h1>
      </section>

      <section class="account-fields-section">
        <!-- Username -->
        <AccountField
          label="username"
          :display-value="state.activeAccount.username"
          :copy-value="state.activeAccount.username"
          :can-copy="!!state.activeAccount.username"
        />

        <!-- Email -->
        <AccountField
          label="email"
          :display-value="state.activeAccount.email || 'No Email'"
          :copy-value="state.activeAccount.email"
          :can-copy="!!state.activeAccount.email"
        />

        <!-- Password -->
        <AccountField
          label="password"
          :display-value="
            !passwordEntropy?.guesses ? 'No Value' : showPassword ? password : '••••••••••••••••'
          "
          :copy-value="fetchPassword"
          :can-copy="!!passwordEntropy?.guesses"
        >
          <template #actions>
            <span v-if="passwordEntropy?.guesses" class="password-strength">
              {{ PASSWORDSTRENGTHS[passwordEntropy.score] }}
            </span>

            <!-- Only show the Eye toggle if a password actually exists -->
            <Button
              v-if="passwordEntropy?.guesses"
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
        <TagList :modelValue="state.activeAccount.tags" />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(state.activeAccount.updated_at)} ` }}</span>
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
  position: relative;
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

  > svg {
    position: absolute;
    right: -1rem;
    bottom: -0.5rem;
  }
}

.display-name {
  font-size: 2rem;
  font-family: var(--font-geo);
}

.account-fields-section {
  display: flex;
  flex-direction: column;
  width: 100%;

  & > *:first-child {
    border-radius: 0.75rem 0.75rem 0 0;
  }

  & > *:last-child {
    border-radius: 0 0 0.75rem 0.75rem;
  }

  & > *:not(:last-child) {
    border-bottom: none;
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
