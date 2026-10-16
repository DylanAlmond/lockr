<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import IconUpload from '../ui/IconUpload.vue';
import useAppStore from '../../stores/appStore.ts';
import { useVault } from '../../composables/useVault.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem } = useAppStore();
const { updateItemSecret } = useVault();

const card = computed(() => (state.activeItem?.kind === 'card' ? state.activeItem : null));

const displayLabel = computed(
  () => card.value?.display_name || card.value?.cardholder_name || 'Untitled'
);
const displayInitial = computed(() => displayLabel.value[0].toUpperCase());

interface CardForm {
  icon: string | null;
  cardholder_name: string;
  brand: string | null;
  expiry_month: string;
  expiry_year: string;
  tags: string[];
}

const form = ref<CardForm>({
  icon: null,
  cardholder_name: '',
  brand: null,
  expiry_month: '',
  expiry_year: '',
  tags: []
});

// Secrets are never prefetched in edit mode. Typing into a secret field
// updates just that field via update_item_secret; leaving it blank keeps
// the stored value.
interface SecretInputs {
  card_number: string;
  security_code: string;
  pin: string;
}
const secretInputs = ref<SecretInputs>({
  card_number: '',
  security_code: '',
  pin: ''
});
const savedSecretInput = ref<SecretInputs>({
  card_number: '',
  security_code: '',
  pin: ''
});

// Fields explicitly marked "clear stored value"; typing into a field
// cancels its clear mark. Applied to values on save.
const clearedSecrets = ref(new Set<string>());

function onSecretInput(key: string, value: string | null) {
  if (value && clearedSecrets.value.has(key)) {
    clearedSecrets.value.delete(key);
  }
}

// Sync local form state whenever the globally active card changes
watch(
  () => card.value,
  (nextCard) => {
    if (nextCard) {
      form.value = {
        icon: nextCard.icon,
        cardholder_name: nextCard.cardholder_name,
        brand: nextCard.brand,
        expiry_month: String(nextCard.expiry_month),
        expiry_year: String(nextCard.expiry_year),
        tags: [...nextCard.tags]
      };
      secretInputs.value = { card_number: '', security_code: '', pin: '' };
      savedSecretInput.value = { card_number: '', security_code: '', pin: '' };
      clearedSecrets.value = new Set();
    }
  },
  { immediate: true }
);

function navigateToDetails() {
  router.push({
    name: route.name as string,
    params: { ...route.params, mode: undefined },
    query: route.query
  });
}

async function handleSave() {
  if (!card.value) return;

  const month = Number(form.value.expiry_month);
  const year = Number(form.value.expiry_year);
  if (!Number.isInteger(month) || month < 1 || month > 12) return;
  if (!Number.isInteger(year) || year < 2000 || year > 9999) return;

  const success = await updateActiveItem({
    icon: form.value.icon,
    tags: form.value.tags,
    data: {
      kind: 'card',
      cardholder_name: form.value.cardholder_name,
      brand: form.value.brand,
      expiry_month: month,
      expiry_year: year
    }
  });
  if (!success) return;

  // Push only the secret fields the user actually touched. The input keys
  // are the backend's field keys verbatim.
  const secretKeys: Array<keyof SecretInputs> = [
    'card_number',
    'security_code',
    'pin'
  ];
  for (const key of secretKeys) {
    const input = secretInputs.value[key].trim();

    if (clearedSecrets.value.has(key)) {
      // Explicit "clear" wins even if the user also typed a replacement
      await updateItemSecret(card.value.vault_id, card.value.id, key, null);
    } else if (input && input !== savedSecretInput.value[key]) {
      await updateItemSecret(card.value.vault_id, card.value.id, key, input);
    }
  }

  navigateToDetails();
}
</script>

<template>
  <!-- Empty -->
  <div v-if="!card" class="wrapper">No card found.</div>

  <!-- Edit Mode -->
  <div v-else class="wrapper">
    <header>
      <span class="editmode-label">Editing</span>

      <nav class="header-toolbar">
        <Button variant="solid" size="small" @click="navigateToDetails">Cancel</Button>
        <Button size="small" @click="handleSave">Save</Button>
      </nav>
    </header>

    <main class="thin-scrollbar">
      <section class="descriptor-section">
        <IconUpload
          v-model="form.icon"
          :fallback-text="displayInitial"
          aria-label="Click to upload card icon"
        />
      </section>

      <section class="card-fields-section">
        <!-- Cardholder Name -->
        <AccountField
          label="cardholder name"
          type="text"
          required
          input
          v-model="form.cardholder_name"
          placeholder="Name on card"
        />

        <!-- Brand -->
        <AccountField
          label="brand"
          type="text"
          input
          v-model="form.brand"
          placeholder="e.g. Visa"
        />

        <!-- Expiry -->
        <AccountField
          label="expiry month"
          type="text"
          input
          v-model="form.expiry_month"
          placeholder="e.g. 07"
        />

        <AccountField
          label="expiry year"
          type="text"
          input
          v-model="form.expiry_year"
          placeholder="e.g. 2030"
        />
      </section>

      <section class="card-secret-section">
        <h2>secrets</h2>

        <div class="secret-hint">
          Leave a field blank to keep its stored value; Clear discards it on save.
        </div>
        <!-- Card Number -->
        <AccountField
          label="card number"
          type="text"
          input
          v-model="secretInputs.card_number"
          :placeholder="clearedSecrets.has('card_number') ? 'will be cleared' : 'unchanged'"
          @update:model-value="(value) => onSecretInput('card_number', value)"
        >
          <template #actions>
            <Button
              :disabled="!secretInputs.card_number.length"
              aria-label="Discard stored card number on save"
              variant="outline"
              size="small"
              @click="clearedSecrets.add('card_number')"
            >
              Clear
            </Button>
          </template>
        </AccountField>

        <!-- Security Code -->
        <AccountField
          label="security code"
          type="text"
          input
          v-model="secretInputs.security_code"
          :placeholder="clearedSecrets.has('security_code') ? 'will be cleared' : 'unchanged'"
          @update:model-value="(value) => onSecretInput('security_code', value)"
        >
          <template #actions>
            <Button
              :disabled="!secretInputs.security_code.length"
              aria-label="Discard stored security code on save"
              variant="outline"
              size="small"
              @click="clearedSecrets.add('security_code')"
            >
              Clear
            </Button>
          </template>
        </AccountField>

        <!-- PIN -->
        <AccountField
          label="pin"
          type="text"
          input
          v-model="secretInputs.pin"
          :placeholder="clearedSecrets.has('pin') ? 'will be cleared' : 'unchanged'"
          @update:model-value="(value) => onSecretInput('pin', value)"
        >
          <template #actions>
            <Button
              :disabled="!secretInputs.pin.length"
              aria-label="Discard stored PIN on save"
              variant="outline"
              size="small"
              @click="clearedSecrets.add('pin')"
            >
              Clear
            </Button>
          </template>
        </AccountField>
      </section>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList v-model="form.tags" editable />
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
  background-color: #efecf9;
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

.card-fields-section,
.card-secret-section {
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

.card-secret-section {
  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
    padding: 0 1.5rem;
  }
}

.secret-hint {
  padding: 0 1.5rem 0.5rem;
  font-size: 0.8125rem;
  color: var(--color-text-tertiary);
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

.editmode-label {
  font-family: var(--font-ui);
  font-size: 1.25rem;
  font-weight: 500;
  color: var(--color-accent-muted);
}
</style>
