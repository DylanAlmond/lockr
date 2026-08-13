<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import { Entropy } from '../../types/index.ts';
import Button from './Button.vue';
import AccountField from './AccountField.vue';
import { useVault } from '../../composables/useVault.ts';
import { PASSWORDSTRENGTHS } from '../../util/constants.ts';
import useAppStore from '../../stores/appStore.ts';
import { Eye, EyeOff } from '@lucide/vue';

const { state, updateActiveAccountPassword } = useAppStore();
const { getPasswordStrength, getSecret } = useVault();

const newPassword = ref<string | null>(null);
const passwordEntropy = ref<Entropy | null>(null);
const showPassword = ref(false);

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

async function fetchPassword() {
  if (!state.activeAccount) return null;

  if (!newPassword.value) {
    newPassword.value = await getSecret(state.activeAccount.vault_id, state.activeAccount.id);
    if (newPassword.value) {
      passwordEntropy.value = await getPasswordStrength(newPassword.value);
    }
  }
  return newPassword.value;
}

async function handleConfirm() {
  if (!state.activeAccount) return;

  await updateActiveAccountPassword(newPassword.value);
  emit('close');
}

// Calculate password strength in real-time as the user types
watch(
  newPassword,
  async (val) => {
    if (!val) {
      passwordEntropy.value = null;
      return;
    }
    passwordEntropy.value = await getPasswordStrength(val);
    console.log(passwordEntropy.value);
  },
  { immediate: true }
);

onMounted(fetchPassword);
</script>

<template>
  <article class="container">
    <header>
      <h2>Change Password</h2>
    </header>

    <main>
      <section class="account-fields-section">
        <AccountField
          label="new password"
          :type="showPassword ? 'text' : 'password'"
          input
          v-model="newPassword"
          placeholder="Enter new password"
        >
          <template #actions>
            <span v-if="passwordEntropy" class="password-strength">
              {{ PASSWORDSTRENGTHS[passwordEntropy.score] }}
            </span>

            <Button
              :disabled="!newPassword?.length"
              aria-label="Toggle password visibility"
              icon-only
              variant="outline"
              size="small"
              :icon-component="showPassword ? EyeOff : Eye"
              @click="showPassword = !showPassword"
            />
          </template>
        </AccountField>
      </section>
    </main>

    <footer>
      <Button variant="outline" @click="emit('close')">Cancel</Button>
      <Button @click="handleConfirm">Save</Button>
    </footer>
  </article>
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

.password-strength {
  margin-right: 0.5rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-green);
  white-space: nowrap;
}

.account-fields-section {
  display: flex;
  flex-direction: column;
  width: 100%;

  > .account-field {
    border-radius: 0.75rem;
  }
}
</style>
