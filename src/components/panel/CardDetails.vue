<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight, Copy, Check, EllipsisVertical, Eye, EyeOff, Pencil, Star } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import { VAULT_COLORS } from '../../util/constants.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import AlertModal from '../ui/AlertModal.vue';
import MoveAccountModal from '../ui/MoveAccountModal.vue';
import { useModal } from '../../composables/useModal.ts';
import {
  useVault
} from '../../composables/useVault.ts';
import {
  SECRET_FIELD_CARD_NUMBER,
  SECRET_FIELD_PIN,
  SECRET_FIELD_SECURITY_CODE
} from '../../util/secrets.ts';
import { markRaw } from 'vue';
import useAppStore from '../../stores/appStore.ts';
import { Vault } from '../../types/index.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem, deleteActiveItem } = useAppStore();
const { getVaultById, getItemSecret } = useVault();
const { openModal } = useModal();

const parentVault = ref<Vault | null>(null);

const card = computed(() => (state.activeItem?.kind === 'card' ? state.activeItem : null));

const displayLabel = computed(
  () => card.value?.display_name || card.value?.cardholder_name || 'Untitled'
);
const displayInitial = computed(() => displayLabel.value[0].toUpperCase());

interface SecretFieldView {
  key: string;
  label: string;
  shown: boolean;
  /** Fetched on demand; never prefetched, never sent elsewhere */
  value: string | null;
  copied: boolean;
}

let copiedTimer: ReturnType<typeof setTimeout> | null = null;

function makeSecretFields(): SecretFieldView[] {
  return [
    { key: SECRET_FIELD_CARD_NUMBER, label: 'card number', shown: false, value: null, copied: false },
    { key: SECRET_FIELD_SECURITY_CODE, label: 'security code', shown: false, value: null, copied: false },
    { key: SECRET_FIELD_PIN, label: 'pin', shown: false, value: null, copied: false }
  ];
}

const secretFields = ref<SecretFieldView[]>(makeSecretFields());

async function toggleSecret(field: SecretFieldView) {
  if (!card.value) return;

  if (field.shown) {
    field.shown = false;
    return;
  }

  if (field.value === null) {
    field.value = await getItemSecret(card.value.vault_id, card.value.id, field.key);
    if (field.value === null) return; // fetch failed — don't flip to a fake show
  }
  field.shown = true;
}

async function copySecret(field: SecretFieldView) {
  if (!card.value) return;

  if (field.value === null) {
    field.value = await getItemSecret(card.value.vault_id, card.value.id, field.key);
    if (field.value === null) return;
  }

  try {
    await navigator.clipboard.writeText(field.value);
    // Reset every other field's copied flag before starting this timer
    if (copiedTimer) {
      clearTimeout(copiedTimer);
      copiedTimer = null;
    }
    secretFields.value.forEach((f) => (f.copied = false));
    field.copied = true;
    copiedTimer = setTimeout(() => {
      field.copied = false;
      copiedTimer = null;
    }, 1500);
  } catch {
    // Clipboard API unavailable — copied flag simply stays false
  }
}

async function toggleFavourite() {
  if (!card.value) return;
  updateActiveItem({ favourite: !card.value.favourite });
}

function goToEdit() {
  if (!card.value) return;
  router.push({
    name: route.name as string,
    params: { ...route.params, accountId: card.value.id, mode: 'edit' },
    query: route.query
  });
}

async function handleDelete() {
  if (!card.value) return;

  const success = await deleteActiveItem();

  if (success) {
    await router.replace({
      name: route.name as string,
      params: { ...route.params, accountId: undefined, mode: undefined },
      query: route.query
    });
  }
}

const miscMenuItems = computed<DropdownItem[]>(() => [
  {
    label: 'Move to Vault',
    disabled: !card.value,
    onSelect: () => {
      if (!card.value) return;
      openModal(markRaw(MoveAccountModal), {
        onClose: () => {},
        onConfirm: () => {}
      });
    }
  },
  {
    label: 'Delete Card',
    onSelect: () => {
      openModal(markRaw(AlertModal), {
        title:
          'Delete ' +
          (card.value?.display_name || card.value?.cardholder_name
            ? `"${displayLabel.value}"`
            : 'Card'),
        message: "Are you sure you want to continue? This can't be undone.",
        actionLabel: 'Delete',
        confirmationValue: card.value?.display_name || card.value?.cardholder_name || undefined,
        onClose: () => {},
        onConfirm: () => handleDelete()
      });
    }
  }
]);

// Clear parent vault and revealed secrets when the active card changes
watch(
  () => [state.activeItem?.id, state.activeItem?.vault_id],
  async () => {
    if (card.value) {
      parentVault.value = await getVaultById(card.value.vault_id);
    } else {
      parentVault.value = null;
    }
    secretFields.value = makeSecretFields();
  },
  { immediate: true }
);
</script>

<template>
  <!-- Empty -->
  <div v-if="!card" class="wrapper">No card found.</div>

  <!-- View Mode -->
  <div v-else class="wrapper">
    <header>
      <div class="vault-label">
        <span
          class="vault-indicator"
          :style="{ backgroundColor: parentVault?.color || VAULT_COLORS[0].hex }"
          aria-hidden="true"
        ></span>
        <span class="vault-name">{{ parentVault?.name || 'Unknown Vault' }}</span>
      </div>

      <nav class="header-toolbar">
        <Button
          aria-label="Favourite"
          icon-only
          variant="outline"
          size="small"
          :icon-component="Star"
          :icon-props="{
            fill: card.favourite ? 'var(--color-accent)' : 'none',
            color: card.favourite ? 'var(--color-accent)' : undefined
          }"
          @click="toggleFavourite"
        />

        <Button variant="outline" size="small" :icon-component="Pencil" @click="goToEdit">
          Edit
        </Button>

        <Dropdown :list="miscMenuItems" #trigger="{ triggerProps }">
          <Button
            class="menu-button"
            aria-label="Card Menu"
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
        <div class="item-icon">
          <img v-if="card.icon" :src="card.icon" alt="Card icon" class="icon-image" />
          <span v-else class="icon-text">{{ displayInitial }}</span>
          <Star
            v-if="card.favourite"
            :size="32"
            :fill="card.favourite ? 'var(--color-accent)' : 'none'"
            color="var(--color-accent)"
          />
        </div>
        <h1 class="display-name">{{ displayLabel }}</h1>
      </section>

      <section class="card-data-section">
        <div class="details-fields">
          <!-- Non-secret fields -->
          <AccountField label="cardholder" :displayValue="card.cardholder_name" />
          <AccountField label="brand" :displayValue="card.brand" />
          <AccountField
            label="expiry"
            :displayValue="`${String(card.expiry_month).padStart(2, '0')}/${card.expiry_year}`"
          />
        </div>

        <!-- Secrets: one field per item, each shown and copied by its own key -->
        <div class="secret-fields">
          <div v-for="field in secretFields" :key="field.key" class="secret-field">
            <div class="field-meta">
              <h2>{{ field.label }}</h2>
              <span v-if="field.shown" class="secret-value">
                {{ field.value ?? 'Unavailable' }}
              </span>
              <span v-else class="secret-value masked">
                •••• •••• ••••
              </span>
            </div>
            <div class="field-actions">
              <Button
                :aria-label="`Toggle ${field.label} visibility`"
                icon-only
                variant="outline"
                size="small"
                :icon-component="field.shown ? EyeOff : Eye"
                @click="toggleSecret(field)"
              />
              <Button
                :aria-label="`Copy ${field.label}`"
                icon-only
                variant="outline"
                size="small"
                :icon-component="field.copied ? Check : Copy"
                :disabled="!field.shown"
                @click="copySecret(field)"
              />
            </div>
          </div>
        </div>
      </section>

      <section class="tags-section">
        <h2>tags</h2>
        <TagList :modelValue="card.tags" />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(card.updated_at)} ` }}</span>
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
  flex: 1;
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
  gap: 0.75rem;

  .vault-indicator {
    width: 1rem;
    height: 1rem;
    aspect-ratio: 1;
    border-radius: 0.25rem;
  }

  .vault-name {
    font-size: 1.125rem;
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

.item-icon {
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

  .icon-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 0.75rem;
  }

  .icon-text {
    pointer-events: none;
  }

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

.card-data-section {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 0rem 0rem;
}

.details-fields,
.secret-fields {
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

.secret-fields {
  margin-top: 1rem;
}

.secret-field {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 1rem 1.5rem;
  overflow: hidden;
  border: 1px solid var(--color-border);
  transition:
    box-shadow 0.2s ease,
    border-color 0.2s ease;

  &:focus-within {
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
  }
}

.field-meta {
  flex: 1;
  min-width: 0;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }
}

.secret-value {
  font: inherit;
  color: var(--color-text);
  overflow-wrap: anywhere;
}

.secret-value.masked {
  color: var(--color-text-muted);
  letter-spacing: 2px;
}

.field-actions {
  display: flex;
  gap: 0.5rem;
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
