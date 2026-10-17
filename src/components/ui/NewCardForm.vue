<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { CreateItemProps, useVault } from '../../composables/useVault.ts';
import {
  SECRET_FIELD_CARD_NUMBER,
  SECRET_FIELD_PIN,
  SECRET_FIELD_SECURITY_CODE
} from '../../util/secrets.ts';
import { SecretInput, Vault } from '../../types/index.ts';
import { cardSecretMask, cardNumberLast4 } from '../../util/secrets.ts';
import ItemField from './ItemField.vue';
import Button from './Button.vue';
import IconUpload from './IconUpload.vue';
import Select from './Select.vue';

const { getUnlockedVaults } = useVault();

const props = defineProps<{ defaultVaultId?: string | null }>();

const emit = defineEmits<{
  (e: 'submit', data: CreateItemProps): void;
  (e: 'close'): void;
}>();

const vaults = ref<Vault[]>([]);

interface CardForm {
  vaultId: string;
  icon: string | null;
  cardholder_name: string;
  brand: string | null;
  expiry_month: string;
  expiry_year: string;
  card_number: string;
  security_code: string;
  pin: string;
}

const form = ref<CardForm>({
  vaultId: '',
  icon: null,
  cardholder_name: '',
  brand: null,
  expiry_month: '',
  expiry_year: '',
  card_number: '',
  security_code: '',
  pin: ''
});

// Secret fields reveal their typed value only while the field is focused;
// blurred they display the value's structural mask — card numbers keep
// their real last 4 digits, derived live from the typed content. An empty
// field shows the 'No Value' placeholder like any other empty field.
const focusedSecret = ref<string | null>(null);

/** Input content: real typed value while focused, otherwise the mask
 * derived from whatever has been typed (nothing → nothing, so the
 * placeholder shows). */
function secretDisplay(key: 'card_number' | 'security_code' | 'pin'): string {
  if (focusedSecret.value === key) return form.value[key];
  const content = form.value[key].trim();
  if (!content) return '';
  const last4 = key === 'card_number' ? cardNumberLast4(content) : null;
  return cardSecretMask(key, true, last4) ?? '';
}

const displayInitial = computed(() => (form.value.cardholder_name || 'C')[0].toUpperCase());

function cardSecrets(): SecretInput[] {
  const secrets: SecretInput[] = [];

  if (form.value.card_number.trim()) {
    secrets.push({ key: SECRET_FIELD_CARD_NUMBER, value: form.value.card_number.trim() });
  }
  if (form.value.security_code.trim()) {
    secrets.push({
      key: SECRET_FIELD_SECURITY_CODE,
      value: form.value.security_code.trim()
    });
  }
  if (form.value.pin.trim()) {
    secrets.push({ key: SECRET_FIELD_PIN, value: form.value.pin.trim() });
  }

  return secrets;
}

function handleSubmit() {
  const { vaultId, icon, cardholder_name, brand, expiry_month, expiry_year } = form.value;
  if (!vaultId || !cardholder_name.trim()) return;

  // Keep the locked contract: month 1–12, a sane year
  const month = Number(expiry_month);
  const year = Number(expiry_year);
  if (!Number.isInteger(month) || month < 1 || month > 12) return;
  if (!Number.isInteger(year) || year < 2000 || year > 9999) return;

  emit('submit', {
    vaultId,
    icon,
    data: {
      kind: 'card',
      cardholder_name,
      brand,
      expiry_month: month,
      expiry_year: year
    },
    secrets: cardSecrets()
  });
}

// Pre-fill the in-context vault once vaults are known
watch(
  () => [vaults.value, props.defaultVaultId],
  () => {
    if (!form.value.vaultId && props.defaultVaultId) {
      form.value.vaultId = props.defaultVaultId;
    }
  },
  { immediate: true }
);

onMounted(async () => {
  vaults.value = await getUnlockedVaults();
});
</script>

<template>
  <form @submit.prevent="handleSubmit">
    <article class="container">
      <header>
        <h2>New Card</h2>
      </header>

      <IconUpload
        v-model="form.icon"
        :fallback-text="displayInitial"
        aria-label="Click to upload card icon"
        hint="Click icon to upload image"
      />

      <main>
        <section class="card-fields-section">
          <div class="select-field">
            <h2>vault</h2>
            <Select
              v-model="form.vaultId"
              :options="vaults.map((v) => ({ value: v.id, label: v.name }))"
              required
              fill
            />
          </div>

          <!-- Cardholder Name -->
          <ItemField
            label="cardholder name"
            type="text"
            required
            input
            v-model="form.cardholder_name"
            placeholder="Name on card"
          />

          <!-- Brand -->
          <ItemField
            label="brand"
            type="text"
            input
            v-model="form.brand"
            placeholder="e.g. Visa"
          />

          <!-- Expiry -->
          <ItemField
            label="expiry month"
            type="text"
            required
            input
            v-model="form.expiry_month"
            placeholder="e.g. 07"
          />

          <ItemField
            label="expiry year"
            type="text"
            required
            input
            v-model="form.expiry_year"
            placeholder="e.g. 2030"
          />

          <!-- Card Number: mask as displayed value, typed value while focused -->
          <ItemField
            label="card number"
            type="text"
            input
            :model-value="secretDisplay('card_number')"
            placeholder="No Value"
            @update:model-value="(value) => (form.card_number = value)"
            @focus="focusedSecret = 'card_number'"
            @blur="focusedSecret = null"
          />

          <!-- Security Code -->
          <ItemField
            label="security code"
            type="text"
            input
            :model-value="secretDisplay('security_code')"
            placeholder="No Value"
            @update:model-value="(value) => (form.security_code = value)"
            @focus="focusedSecret = 'security_code'"
            @blur="focusedSecret = null"
          />

          <!-- PIN -->
          <ItemField
            label="pin"
            type="text"
            input
            :model-value="secretDisplay('pin')"
            placeholder="No Value"
            @update:model-value="(value) => (form.pin = value)"
            @focus="focusedSecret = 'pin'"
            @blur="focusedSecret = null"
          />
        </section>
      </main>

      <footer>
        <Button variant="outline" @click="emit('close')">Cancel</Button>
        <Button type="submit">Create</Button>
      </footer>
    </article>
  </form>
</template>

<style scoped>
.container {
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
  color: var(--color-text-secondary);
  line-height: 1.5rem;
  width: 100%;
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
  width: 100%;

  /* Pinned inside the modal's scrollport so the buttons stay visible while
     content scrolls underneath; the opaque background masks passing fields */
  position: sticky;
  bottom: 0;
  background-color: var(--color-bg);
}

.card-fields-section {
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

.select-field {
  display: flex;
  flex-direction: column;
  padding: 1rem 1.5rem;
  overflow: hidden;
  border: 1px solid var(--color-border);
  transition:
    box-shadow 0.2s ease,
    border-color 0.2s ease;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }

  &:focus-within {
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
  }
}
</style>
