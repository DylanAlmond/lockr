<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight, EllipsisVertical, Eye, EyeOff, Pencil, Star } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import ItemField from '../ui/ItemField.vue';
import { VAULT_COLORS } from '../../util/constants.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import AlertModal from '../ui/AlertModal.vue';
import MoveItemModal from '../ui/MoveItemModal.vue';
import { useModal } from '../../composables/useModal.ts';
import { useVault } from '../../composables/useVault.ts';
import { cardSecretMask, type CardSecretKey, SECRET_FIELD_CARD_NUMBER, SECRET_FIELD_PIN, SECRET_FIELD_SECURITY_CODE } from '../../util/secrets.ts';
import { markRaw } from 'vue';
import useAppStore from '../../stores/appStore.ts';
import { Vault } from '../../types/index.ts';
import { useSecretReveal, type SecretReveal } from '../../composables/useSecretReveal.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem, deleteActiveItem } = useAppStore();
const { getVaultById, getItemSecret, copyItemSecret } = useVault();
const { openModal } = useModal();

const parentVault = ref<Vault | null>(null);

const card = computed(() => (state.activeItem?.kind === 'card' ? state.activeItem : null));

const displayLabel = computed(
  () => card.value?.display_name || card.value?.cardholder_name || 'Untitled'
);
const displayInitial = computed(() => displayLabel.value[0].toUpperCase());

const cardNumberReveal = useSecretReveal();
const securityCodeReveal = useSecretReveal();
const pinReveal = useSecretReveal();

interface SecretFieldView {
  key: CardSecretKey;
  label: string;
  reveal: SecretReveal;
  /** Copy through the core; the value never enters the renderer. */
  copyAction: () => Promise<boolean>;
}

const secretFields: SecretFieldView[] = [
  {
    key: SECRET_FIELD_CARD_NUMBER,
    label: 'card number',
    reveal: cardNumberReveal,
    copyAction: () => copySecret(SECRET_FIELD_CARD_NUMBER)
  },
  {
    key: SECRET_FIELD_SECURITY_CODE,
    label: 'security code',
    reveal: securityCodeReveal,
    copyAction: () => copySecret(SECRET_FIELD_SECURITY_CODE)
  },
  {
    key: SECRET_FIELD_PIN,
    label: 'pin',
    reveal: pinReveal,
    copyAction: () => copySecret(SECRET_FIELD_PIN)
  }
];

// Copy/-eye affordances exist only for fields that have a value
function secretPresent(key: string): boolean {
  return !!card.value?.secrets_present.includes(key);
}

// Masked placeholder echoes the value's structure via the shared helper
// (card number keeps the real last 4); absent fields resolve to null →
// ItemField shows 'No Value'.
function secretDisplay(field: SecretFieldView): string | null {
  if (field.reveal.isRevealed.value) return field.reveal.secret.value;
  return cardSecretMask(field.key, secretPresent(field.key), card.value?.card_number_last4 ?? null);
}

async function copySecret(key: string): Promise<boolean> {
  if (!card.value) return false;

  return copyItemSecret(card.value.vault_id, card.value.id, key);
}

async function fetchSecret(key: string): Promise<string | null> {
  if (!card.value) return null;

  try {
    return await getItemSecret(card.value.vault_id, card.value.id, key);
  } catch {
    // Fetch failed: keep the field masked / 'No Value' — same soft
    // behaviour as the pre-throwing lookup
    return null;
  }
}

async function toggleSecret(field: SecretFieldView) {
  // Only fetch if the key is known to have a value
  if (!secretPresent(field.key)) return;

  await field.reveal.toggle(() => fetchSecret(field.key));
}

async function toggleFavourite() {
  if (!card.value) return;
  updateActiveItem({ favourite: !card.value.favourite });
}

function goToEdit() {
  if (!card.value) return;
  router.push({
    name: route.name as string,
    params: { ...route.params, itemId: card.value.id, mode: 'edit' },
    query: route.query
  });
}

async function handleDelete() {
  if (!card.value) return;

  const success = await deleteActiveItem();

  if (success) {
    await router.replace({
      name: route.name as string,
      params: { ...route.params, itemId: undefined, mode: undefined },
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
      openModal(markRaw(MoveItemModal), {
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

// Clear parent vault and revealed secrets when the active card changes —
// keying on updated_at means metadata-only saves collapse reveals too.
// Intended: this watch is the shared "item mutated" signal (same shape as
// AccountDetails' password reset), and a post-save refresh is expected to
// re-collapse. Cross-panel watch-signal unification is part of #27's
// item-contract consolidation.
watch(
  () => [state.activeItem?.id, state.activeItem?.vault_id, state.activeItem?.updated_at],
  async () => {
    if (card.value) {
      parentVault.value = await getVaultById(card.value.vault_id);
    } else {
      parentVault.value = null;
    }
    for (const field of secretFields) field.reveal.hide();
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
          <ItemField label="cardholder" :displayValue="card.cardholder_name" />
          <ItemField label="brand" :displayValue="card.brand" />
          <ItemField
            label="expiry"
            :displayValue="`${String(card.expiry_month).padStart(2, '0')}/${card.expiry_year}`"
          />
        </div>

        <!-- Secrets: plaintext rows with account-password affordances -->
        <div class="secret-fields">
          <ItemField
            v-for="field in secretFields"
            :key="field.key"
            :label="field.label"
            :display-value="secretDisplay(field)"
            :copy-action="field.copyAction"
            :can-copy="secretPresent(field.key)"
          >
            <template #actions>
              <Button
                v-if="secretPresent(field.key)"
                :aria-label="`Toggle ${field.label} visibility`"
                icon-only
                variant="outline"
                size="small"
                :icon-component="field.reveal.isRevealed.value ? EyeOff : Eye"
                @click="toggleSecret(field)"
              />
            </template>
          </ItemField>
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
