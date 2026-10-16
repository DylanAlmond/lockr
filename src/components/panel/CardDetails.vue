<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight, EllipsisVertical, Eye, EyeOff, Pencil, Star } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import { VAULT_COLORS } from '../../util/constants.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import AlertModal from '../ui/AlertModal.vue';
import MoveAccountModal from '../ui/MoveAccountModal.vue';
import { useModal } from '../../composables/useModal.ts';
import { useVault } from '../../composables/useVault.ts';
import { cardSecretMask, SECRET_FIELD_CARD_NUMBER, SECRET_FIELD_PIN, SECRET_FIELD_SECURITY_CODE } from '../../util/secrets.ts';
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
  /** Stable per-field copy hook (AccountField resets on prop identity change) */
  copyValue: () => Promise<string | null>;
}

function fetchSecretByKey(key: string): Promise<string | null> {
  const field = secretFields.value.find((f) => f.key === key);
  return field ? fetchSecret(field) : Promise.resolve(null);
}

function makeSecretFields(): SecretFieldView[] {
  const make = (key: string, label: string): SecretFieldView => ({
    key,
    label,
    shown: false,
    value: null,
    copyValue: () => fetchSecretByKey(key)
  });

  return [
    make(SECRET_FIELD_CARD_NUMBER, 'card number'),
    make(SECRET_FIELD_SECURITY_CODE, 'security code'),
    make(SECRET_FIELD_PIN, 'pin')
  ];
}

const secretFields = ref<SecretFieldView[]>(makeSecretFields());

// Copy/-eye affordances exist only for fields that have a value
function secretPresent(key: string): boolean {
  return !!card.value?.secrets_present.includes(key);
}

// Masked placeholder echoes the value's structure via the shared helper
// (card number keeps the real last 4); absent fields resolve to null →
// AccountField shows 'No Value'.
function secretDisplay(field: SecretFieldView): string | null {
  if (field.shown) return field.value ?? null; // failed fetch → 'No Value'
  return cardSecretMask(field.key, secretPresent(field.key), card.value?.card_number_last4 ?? null);
}

async function fetchSecret(field: SecretFieldView): Promise<string | null> {
  if (!card.value) return null;
  if (field.value === null) {
    field.value = await getItemSecret(card.value.vault_id, card.value.id, field.key);
  }
  return field.value;
}

async function toggleSecret(field: SecretFieldView) {
  if (field.shown) {
    field.shown = false;
    return;
  }
  // Only fetch if the key is known to have a value
  if (!secretPresent(field.key)) return;

  await fetchSecret(field);
  if (field.value !== null) {
    field.shown = true;
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
// (parity with AccountDetails' password reset on any mutation)
watch(
  () => [state.activeItem?.id, state.activeItem?.vault_id, state.activeItem?.updated_at],
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

        <!-- Secrets: plaintext rows with account-password affordances -->
        <div class="secret-fields">
          <AccountField
            v-for="field in secretFields"
            :key="field.key"
            :label="field.label"
            :display-value="secretDisplay(field)"
            :copy-value="field.copyValue"
            :can-copy="secretPresent(field.key)"
          >
            <template #actions>
              <Button
                v-if="secretPresent(field.key)"
                :aria-label="`Toggle ${field.label} visibility`"
                icon-only
                variant="outline"
                size="small"
                :icon-component="field.shown ? EyeOff : Eye"
                @click="toggleSecret(field)"
              />
            </template>
          </AccountField>
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
