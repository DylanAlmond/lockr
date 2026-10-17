<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { CreateItemProps, useVault } from '../../composables/useVault.ts';
import { Vault } from '../../types/index.ts';
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

interface IdentityForm {
  vaultId: string;
  icon: string | null;
  full_name: string;
  email: string | null;
  phone: string | null;
  address_line1: string | null;
  address_line2: string | null;
  city: string | null;
  postal_code: string | null;
  country: string | null;
}

const form = ref<IdentityForm>({
  vaultId: '',
  icon: null,
  full_name: '',
  email: null,
  phone: null,
  address_line1: null,
  address_line2: null,
  city: null,
  postal_code: null,
  country: null
});

const displayInitial = computed(() => (form.value.full_name || 'I')[0].toUpperCase());

function handleSubmit() {
  const { vaultId, icon, ...fields } = form.value;
  if (!vaultId || !fields.full_name.trim()) return;

  emit('submit', {
    vaultId,
    icon,
    data: {
      kind: 'identity',
      ...fields
    }
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
        <h2>New Identity</h2>
      </header>

      <IconUpload
        v-model="form.icon"
        :fallback-text="displayInitial"
        aria-label="Click to upload identity icon"
        hint="Click icon to upload image"
      />

      <main>
        <section class="identity-fields-section">
          <div class="select-field">
            <h2>vault</h2>
            <Select
              v-model="form.vaultId"
              :options="vaults.map((v) => ({ value: v.id, label: v.name }))"
              required
              fill
            />
          </div>

          <!-- Full Name -->
          <ItemField
            label="full name"
            type="text"
            required
            input
            v-model="form.full_name"
            placeholder="Full Name"
          />

          <!-- Email -->
          <ItemField
            label="email"
            type="email"
            input
            v-model="form.email"
            placeholder="email@example.com"
          />

          <!-- Phone -->
          <ItemField
            label="phone"
            type="tel"
            input
            v-model="form.phone"
            placeholder="+1 555 0000"
          />

          <!-- Address -->
          <ItemField
            label="address line 1"
            type="text"
            input
            v-model="form.address_line1"
            placeholder="Street Address"
          />

          <ItemField
            label="address line 2"
            type="text"
            input
            v-model="form.address_line2"
            placeholder="Apartment, suite, unit…"
          />

          <ItemField
            label="city"
            type="text"
            input
            v-model="form.city"
            placeholder="City"
          />

          <ItemField
            label="postal code"
            type="text"
            input
            v-model="form.postal_code"
            placeholder="Postal Code"
          />

          <ItemField
            label="country"
            type="text"
            input
            v-model="form.country"
            placeholder="Country"
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
     content scrolls underneath; the opaque background masks passing fields.
     The modal shell has no bottom padding — this footer owns the modal's
     bottom spacing, so padding-bottom keeps the pinned background flush
     with the shell's bottom edge */
  position: sticky;
  bottom: 0;
  padding-bottom: 1.5rem;
  background-color: var(--color-bg);
}

.identity-fields-section {
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
