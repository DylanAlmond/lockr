<script setup lang="ts">
import { onMounted, ref, watch } from 'vue';
import { Entropy, Vault } from '../../types/index.ts';
import Button from './Button.vue';
import AccountField from './AccountField.vue';
import { CreateAccountProps, useVault } from '../../composables/useVault.ts';
import { PASSWORDSTRENGTHS } from '../../util/constants.ts';
import { useRouter } from 'vue-router';

const { getPasswordStrength, getUnlockedVaults, addAccount } = useVault();
const router = useRouter();

const vaults = ref<Vault[]>([]);

const form = ref<CreateAccountProps>({
  vaultId: '',
  username: '',
  password: ''
});

const passwordEntropy = ref<Entropy | null>(null);

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

async function handleConfirm() {
  if (!form.value) return;

  const account = await addAccount(form.value);

  if (account) {
    router.push({
      name: 'vault',
      params: {
        vaultId: account.vault_id,
        accountId: account.id
      }
    });
  }

  emit('close');
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
  }
);

onMounted(async () => {
  vaults.value = await getUnlockedVaults();
});
</script>

<template>
  <form @submit.prevent="handleConfirm">
    <article class="container">
      <header>
        <h2>New Account</h2>
      </header>

      <main>
        <section class="account-fields-section">
          <select v-model="form.vaultId" required>
            <option value="" disabled>Select a vault</option>

            <option v-for="vault in vaults" :key="vault.id" :value="vault.id">
              {{ vault.name }}
            </option>
          </select>

          <!-- Display Name -->
          <AccountField label="display name" type="text" input v-model="form.displayName" />

          <!-- Username -->
          <AccountField label="username" type="text" required input v-model="form.username" />

          <!-- Email -->
          <AccountField label="email" type="email" input v-model="form.email" />

          <!-- Password -->
          <AccountField
            label="password"
            type="password"
            input
            v-model="form.password"
            placeholder="Enter new password"
          >
            <template #actions>
              <span v-if="passwordEntropy !== null" class="password-strength">
                {{ PASSWORDSTRENGTHS[passwordEntropy.score] }}
              </span>
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
</style>
