<script setup lang="ts">
import { computed, ref } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { CreditCard, FileText, IdCard, KeyRound } from '@lucide/vue';
import Button from './Button.vue';
import NewAccountForm from './NewAccountForm.vue';
import NewCardForm from './NewCardForm.vue';
import NewIdentityForm from './NewIdentityForm.vue';
import NewNoteForm from './NewNoteForm.vue';
import { CreateItemProps } from '../../composables/useVault.ts';
import useAppStore from '../../stores/appStore.ts';

type ItemType = 'account' | 'card' | 'note' | 'identity';

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const route = useRoute();
const router = useRouter();

const props = defineProps<{
  /** Pre-fill the in-context vault when opened inside a vault view */
  defaultVaultId?: string | null;
}>();

const { createNewItem } = useAppStore();

const selectedType = ref<ItemType | null>(null);

function selectType(type: { disabled: boolean; value: ItemType }) {
  if (!type.disabled) selectedType.value = type.value;
}

const types = computed(() => [
  { value: 'account' as ItemType, label: 'Account', description: 'Logins, emails, passwords', icon: KeyRound, disabled: false },
  { value: 'card' as ItemType, label: 'Card', description: 'Bank Cards, Credit Cards', icon: CreditCard, disabled: false },
  { value: 'note' as ItemType, label: 'Note', description: 'Plain note with a body', icon: FileText, disabled: false },
  { value: 'identity' as ItemType, label: 'Identity', description: 'Names, addresses, contacts', icon: IdCard, disabled: false }
]);

async function handleAccountSubmit(data: CreateItemProps) {
  const account = await createNewItem(data);
  if (account) {
    navigateToCreated(account.vault_id, account.id);
  }
  emit('close');
}

async function handleNoteSubmit(data: CreateItemProps) {
  const note = await createNewItem(data);
  if (note) {
    navigateToCreated(note.vault_id, note.id);
  }
  emit('close');
}

async function handleIdentitySubmit(data: CreateItemProps) {
  const identity = await createNewItem(data);
  if (identity) {
    navigateToCreated(identity.vault_id, identity.id);
  }
  emit('close');
}

async function handleCardSubmit(data: CreateItemProps) {
  const card = await createNewItem(data);
  if (card) {
    navigateToCreated(card.vault_id, card.id);
  }
  emit('close');
}

function navigateToCreated(vaultId: string, itemId: string) {
  router.push({
    name: 'vault',
    params: {
      vaultId,
      itemId: itemId
    }
  });
}
</script>

<template>
  <div v-if="!selectedType" class="picker-container">
    <header>
      <h2>New Item</h2>
    </header>

    <main>
      <button
        v-for="type in types"
        :key="type.value"
        type="button"
        class="type-option"
        :disabled="type.disabled"
        :aria-disabled="type.disabled"
        @click="selectType(type)"
      >
        <span class="type-icon">
          <component :is="type.icon" :size="22" />
        </span>
        <span class="type-text">
          <span class="type-label">{{ type.label }}</span>
          <span class="type-description">{{ type.description }}</span>
        </span>
      </button>
    </main>

    <footer>
      <Button variant="outline" @click="emit('close')">Cancel</Button>
    </footer>
  </div>

  <NewAccountForm
    v-else-if="selectedType === 'account'"
    :default-vault-id="props.defaultVaultId ?? (route.params.vaultId as string) ?? null"
    @submit="handleAccountSubmit"
    @close="emit('close')"
  />

  <NewNoteForm
    v-else-if="selectedType === 'note'"
    :default-vault-id="props.defaultVaultId ?? (route.params.vaultId as string) ?? null"
    @submit="handleNoteSubmit"
    @close="emit('close')"
  />

  <NewIdentityForm
    v-else-if="selectedType === 'identity'"
    :default-vault-id="props.defaultVaultId ?? (route.params.vaultId as string) ?? null"
    @submit="handleIdentitySubmit"
    @close="emit('close')"
  />

  <NewCardForm
    v-else-if="selectedType === 'card'"
    :default-vault-id="props.defaultVaultId ?? (route.params.vaultId as string) ?? null"
    @submit="handleCardSubmit"
    @close="emit('close')"
  />
</template>

<style scoped>
.picker-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1.5rem;
  width: 100%;
}

header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 0.5rem;
  width: 100%;
}

header > h2 {
  font-size: 1.5rem;
  font-family: var(--font-geo);
  text-align: center;
  width: 100%;
}

main {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  width: 100%;
  color: var(--color-text-secondary);
}

.type-option {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 0.875rem 1.25rem;
  font: inherit;
  text-align: left;
  color: inherit;
  background: transparent;
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
  cursor: pointer;
  transition:
    background-color 0.2s ease,
    border-color 0.2s ease,
    box-shadow 0.2s ease;

  &:hover:not(:disabled) {
    background-color: var(--color-hover);
  }

  &:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  &:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
  }
}

.type-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 2.75rem;
  height: 2.75rem;
  border-radius: 0.5rem;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);
  flex-shrink: 0;
}

.type-text {
  display: flex;
  flex-direction: column;
  gap: 0.125rem;
  min-width: 0;
}

.type-label {
  font-family: var(--font-geo);
  font-weight: 550;
  color: var(--color-text);
}

.type-description {
  font-size: 0.875rem;
  color: var(--color-text-tertiary);
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
  width: 100%;
}
</style>
