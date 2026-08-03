<script setup lang="ts">
import { Account, Entropy, Vault } from '../../types';
import { ChevronRight, EllipsisVertical, Eye, EyeOff, Lock, Pencil, Star } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import { PASSWORDSTRENGTHS } from '../../util/constants.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import { ref } from 'vue';
import AlertModal from '../ui/AlertModal.vue';
import { useModal } from '../../composables/useModal.ts';
import { useVault } from '../../composables/useVault.ts';
import { useRouter } from 'vue-router';

interface Props {
  account: Account | null;
  vault: Vault | null;
  passwordEntropy: Entropy | null;
  password: string | null;
  showPassword: boolean;
  fetchPassword: () => Promise<string | null>;
}

const { openModal } = useModal();
const { deleteAccount } = useVault();
const router = useRouter();

const props = defineProps<Props>();

const emit = defineEmits<{
  (e: 'togglePassword'): void;
  (e: 'toggleFavourite'): void;
  (e: 'edit'): void;
}>();

async function handleDelete() {
  console.log('hello');

  if (!props.account?.vault_id || !props.account.id) return;

  const success = await deleteAccount(props.account.vault_id, props.account.id);
  console.log(success);

  if (success) {
    await router.replace({
      name: router.currentRoute.value.name as string,
      params: {
        ...router.currentRoute.value.params,
        passwordId: null
      }
    });
  }
}

const miscMenuItems = ref<DropdownItem[]>([
  {
    label: 'Change Password',
    onSelect: () => {
      console.log('Create vault!');
    }
  },
  {
    label: 'Delete Account',
    onSelect: () => {
      openModal(AlertModal, {
        title:
          'Delete ' + (props.account?.display_name ? `"${props.account.display_name}"` : 'Account'),
        message: "Are you sure you want to continue? This can't be undone.",
        actionLabel: 'Delete',
        onClose: () => {},
        onConfirm: () => handleDelete()
      });
    }
  }
]);
</script>

<template>
  <!-- Empty -->
  <div v-if="!account" class="wrapper">No account found.</div>

  <!-- View Mode -->
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
          :icon-props="{
            fill: account.favourite ? 'var(--color-accent)' : 'none',
            color: account.favourite ? 'var(--color-accent)' : undefined
          }"
          @click="emit('toggleFavourite')"
        />

        <Button variant="outline" size="small" :icon-component="Pencil" @click="emit('edit')"
          >Edit</Button
        >

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
          {{ (account.display_name || account.username)[0].toUpperCase() }}

          <Star
            v-if="account.favourite"
            :size="32"
            :fill="account.favourite ? 'var(--color-accent)' : undefined"
          />
        </div>
        <h1 class="display-name">{{ account.display_name }}</h1>
      </section>

      <section class="account-fields-section">
        <!-- Username -->
        <AccountField
          label="username"
          :display-value="account.username"
          :copy-value="account.username"
          can-copy
        />

        <!-- Email -->
        <AccountField
          label="email"
          :display-value="account.email || 'No Email'"
          :copy-value="account.email"
          can-copy
        />

        <!-- Password -->
        <AccountField
          label="password"
          :display-value="showPassword ? password || '••••••••••••••••' : '••••••••••••••••'"
          :copy-value="fetchPassword"
          can-copy
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
              @click="emit('togglePassword')"
            />
          </template>
        </AccountField>
      </section>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList :modelValue="account.tags" />
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
