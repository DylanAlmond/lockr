<script setup lang="ts">
import { onMounted, ref, computed, watch } from 'vue';
import { Entropy, Vault } from '../../types/index.ts';
import Button from './Button.vue';
import ItemField from './ItemField.vue';
import { CreateItemProps, useVault } from '../../composables/useVault.ts';
import { PASSWORDSTRENGTHS } from '../../util/constants.ts';
// import { fetchBrandLogoAsBase64 } from '../../util/imageUpload.ts';
import Select from './Select.vue';
import IconUpload from './IconUpload.vue';

const { getPasswordStrength, getUnlockedVaults } = useVault();

const vaults = ref<Vault[]>([]);
const manuallySetIcon = ref(false);

interface AccountForm {
  vaultId: string;
  username: string;
  password: string;
  displayName?: string | null;
  email?: string | null;
  icon?: string | null;
}

const form = ref<AccountForm>({
  vaultId: '',
  username: '',
  password: ''
});

const props = defineProps<{ defaultVaultId?: string | null }>();

const passwordEntropy = ref<Entropy | null>(null);

// Secret fields reveal their value only while the field is focused;
// on blur they re-mask as password dots.
const focusedSecret = ref<string | null>(null);

function secretType(key: string): 'text' | 'password' {
  return focusedSecret.value === key ? 'text' : 'password';
}

const displayInitial = computed(() => {
  return (form.value.displayName || form.value.username || '')[0]?.toUpperCase() || '?';
});

const emit = defineEmits<{
  (e: 'submit', data: CreateItemProps): void;
  (e: 'close'): void;
}>();

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

function handleSubmit() {
  if (!form.value.vaultId || !form.value.username) return;
  // The account kind rides the generic item surface; the initial password
  // is a `password` secret field
  emit('submit', {
    vaultId: form.value.vaultId,
    displayName: form.value.displayName ?? null,
    icon: form.value.icon ?? null,
    data: {
      kind: 'account',
      username: form.value.username,
      email: form.value.email ?? null
    },
    secrets: [{ key: 'password', value: form.value.password }]
  });
}

// Calculate password strength in real-time as the user types
watch(
  () => form.value.password,
  async (val) => {
    if (!val) {
      passwordEntropy.value = null;
      return;
    }
    passwordEntropy.value = await getPasswordStrength(val);
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
        <h2>New Account</h2>
      </header>

      <IconUpload
        v-model="form.icon"
        :fallback-text="displayInitial"
        aria-label="Click to upload account icon"
        hint="Click icon to upload image"
        @update:model-value="manuallySetIcon = true"
      />

      <main>
        <section class="item-fields-section">
          <div class="select-field">
            <h2 id="move-vault-label">vault</h2>
            <Select
              v-model="form.vaultId"
              :options="vaults.map((v) => ({ value: v.id, label: v.name }))"
              required
              fill
            />
          </div>

          <!-- Display Name -->
          <ItemField
            label="display name"
            type="text"
            input
            v-model="form.displayName"
            placeholder="Display Name"
          />

          <!-- Username -->
          <ItemField
            label="username"
            type="text"
            required
            input
            v-model="form.username"
            placeholder="Username"
          />

          <!-- Email -->
          <ItemField
            label="email"
            type="email"
            input
            v-model="form.email"
            placeholder="email@example.com"
          />

          <!-- Password -->
          <ItemField
            label="password"
            :type="secretType('password')"
            input
            v-model="form.password"
            placeholder="No Value"
            @focus="focusedSecret = 'password'"
            @blur="focusedSecret = null"
          >
            <template #actions>
              <span v-if="passwordEntropy !== null" class="password-strength">
                {{ PASSWORDSTRENGTHS[passwordEntropy.score] }}
              </span>
            </template>
          </ItemField>
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

.password-strength {
  margin-right: 0.5rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-green);
  white-space: nowrap;
}

.item-fields-section {
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
