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
import { cardSecretMask, cardNumberLast4, SECRET_FIELD_CARD_NUMBER } from '../../util/secrets.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem, setActiveItem } = useAppStore();
const { getItemSecret, updateItemSecret } = useVault();

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

// Secrets are fetched on focus: an unfocused field renders the stored
// value's structural mask as its displayed content (card numbers keep
// their real last 4), while focusing reveals the real value and blurring
// re-masks. Input type is always text so the mask structure is visible.
type SecretKey = 'card_number' | 'security_code' | 'pin';

const SECRET_KEYS: SecretKey[] = ['card_number', 'security_code', 'pin'];

interface SecretState {
  /** Value as fetched ('' when the key has none); null until fetched */
  original: string | null;
  input: string;
  fetched: boolean;
}

function freshSecretStates(): Record<SecretKey, SecretState> {
  return {
    card_number: { original: null, input: '', fetched: false },
    security_code: { original: null, input: '', fetched: false },
    pin: { original: null, input: '', fetched: false }
  };
}

const secretStates = ref<Record<SecretKey, SecretState>>(freshSecretStates());
const focusedSecret = ref<SecretKey | null>(null);

async function onSecretFocus(key: SecretKey) {
  focusedSecret.value = key;
  const state = secretStates.value[key];
  if (state.fetched || !card.value) return;

  // The fetch may outlive this card's context; discard if it changed
  const targetId = card.value.id;
  const stored = await getItemSecret(card.value.vault_id, card.value.id, key);
  if (!card.value || card.value.id !== targetId) return;

  const fresh = secretStates.value[key];
  fresh.original = stored ?? ''; // '' == this key has no stored value
  fresh.input = stored ?? '';
  fresh.fetched = true;
}

function onSecretBlur(key: SecretKey) {
  if (focusedSecret.value === key) {
    focusedSecret.value = null;
  }
}

/** Structural mask derived from typed/stored content */
function secretMask(key: SecretKey, content: string): string {
  if (key === 'card_number') {
    return cardSecretMask(SECRET_FIELD_CARD_NUMBER, true, cardNumberLast4(content)) ?? '';
  }
  return cardSecretMask(key, true, null) ?? '';
}

/** Structural mask from payload-only knowledge (before the field is
 * fetched): the card number keeps the derived last 4, others mask fully. */
function payloadMask(key: SecretKey): string {
  if (key === 'card_number') {
    return cardSecretMask(
      SECRET_FIELD_CARD_NUMBER,
      true,
      card.value?.card_number_last4 ?? null
    ) ?? '';
  }
  return cardSecretMask(key, true, null) ?? '';
}

function secretPresent(key: SecretKey): boolean {
  return !!card.value?.secrets_present.includes(key);
}

/** Input content: the real value while focused (including mid-typing),
 * otherwise the structural mask — or nothing for an absent key, leaving
 * the 'No Value' placeholder visible. */
function secretDisplay(key: SecretKey): string {
  const s = secretStates.value[key];
  if (focusedSecret.value === key) return s.input;
  if (s.fetched) return s.input ? secretMask(key, s.input) : '';
  return secretPresent(key) ? payloadMask(key) : '';
}

// Placeholder only surfaces when the displayed content is empty: a stored
// value shows its mask, a fetched-and-emptied field (the user is clearing
// it) and absent keys show 'No Value'.
function secretPlaceholder(key: SecretKey): string {
  const s = secretStates.value[key];
  if (s.fetched && !s.input) return 'No Value';
  return secretPresent(key) ? payloadMask(key) : 'No Value';
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
      secretStates.value = freshSecretStates();
      focusedSecret.value = null;
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

  // Snapshot the secret states FIRST: pushing metadata replaces the
  // active item object, which re-triggers the reset watcher below and
  // would otherwise wipe the fetched values before the diff loop runs.
  const snapshot = SECRET_KEYS.map((key) => ({ key, ...secretStates.value[key] }));

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

  // Push only the secret fields the user actually opened and edited,
  // from the pre-save snapshot: typed → update; emptied from a value →
  // clear; unchanged → no call. Emptying a secret removes the value,
  // like any other field.
  let secretsTouched = false;
  for (const { key, original, input, fetched } of snapshot) {
    if (!fetched || !card.value) continue;

    const value = input.trim();
    if (value !== original) {
      await updateItemSecret(card.value.vault_id, card.value.id, key, value || null);
      secretsTouched = true;
    }
  }

  // Secret changes re-derive the payload (presence flags, last4); refresh
  // the active item so the details view shows the final state.
  if (secretsTouched && card.value) {
    await setActiveItem(card.value.id);
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

      <!-- Secret fields (no heading; value revealed only while focused) -->
      <section class="card-secret-section">
        <!-- Card Number: mask as displayed value, real value while focused -->
        <AccountField
          label="card number"
          type="text"
          input
          :model-value="secretDisplay('card_number')"
          :placeholder="secretPlaceholder('card_number')"
          @update:model-value="(value) => (secretStates.card_number.input = value)"
          @focus="onSecretFocus('card_number')"
          @blur="onSecretBlur('card_number')"
        />

        <!-- Security Code -->
        <AccountField
          label="security code"
          type="text"
          input
          :model-value="secretDisplay('security_code')"
          :placeholder="secretPlaceholder('security_code')"
          @update:model-value="(value) => (secretStates.security_code.input = value)"
          @focus="onSecretFocus('security_code')"
          @blur="onSecretBlur('security_code')"
        />

        <!-- PIN -->
        <AccountField
          label="pin"
          type="text"
          input
          :model-value="secretDisplay('pin')"
          :placeholder="secretPlaceholder('pin')"
          @update:model-value="(value) => (secretStates.pin.input = value)"
          @focus="onSecretFocus('pin')"
          @blur="onSecretBlur('pin')"
        />
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
  display: flex;
  flex-direction: column;
  width: 100%;
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
