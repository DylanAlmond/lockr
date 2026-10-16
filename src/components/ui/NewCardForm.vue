<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { Eye, EyeOff } from '@lucide/vue';
import { CreateItemProps, useVault } from '../../composables/useVault.ts';
import {
  SECRET_FIELD_CARD_NUMBER,
  SECRET_FIELD_PIN,
  SECRET_FIELD_SECURITY_CODE
} from '../../util/secrets.ts';
import { SecretInput, Vault } from '../../types/index.ts';
import AccountField from './AccountField.vue';
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

// Store the numbers as text, mask the ones the user won't be reading as
// they type: the card number stays visible, the codes start hidden.
const showSecurityCode = ref(false);
const showPin = ref(false);

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
            required
            input
            v-model="form.expiry_month"
            placeholder="e.g. 07"
          />

          <AccountField
            label="expiry year"
            type="text"
            required
            input
            v-model="form.expiry_year"
            placeholder="e.g. 2030"
          />

          <!-- Card Number (secret, visible while typing) -->
          <AccountField
            label="card number"
            type="text"
            input
            v-model="form.card_number"
            placeholder="4111 1111 1111 1234"
          />

          <!-- Security Code (secret, masked) -->
          <AccountField
            label="security code"
            :type="showSecurityCode ? 'text' : 'password'"
            input
            v-model="form.security_code"
            placeholder="CVC"
          >
            <template #actions>
              <Button
                :disabled="!form.security_code.length"
                aria-label="Toggle security code visibility"
                icon-only
                variant="outline"
                size="small"
                :icon-component="showSecurityCode ? EyeOff : Eye"
                @click="showSecurityCode = !showSecurityCode"
              />
            </template>
          </AccountField>

          <!-- PIN (secret, masked) -->
          <AccountField
            label="pin"
            :type="showPin ? 'text' : 'password'"
            input
            v-model="form.pin"
            placeholder="4-digit PIN"
          >
            <template #actions>
              <Button
                :disabled="!form.pin.length"
                aria-label="Toggle PIN visibility"
                icon-only
                variant="outline"
                size="small"
                :icon-component="showPin ? EyeOff : Eye"
                @click="showPin = !showPin"
              />
            </template>
          </AccountField>
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
