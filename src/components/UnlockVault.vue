<script setup lang="ts">
import { ref } from 'vue';
import { useVault } from '../composables/useVault';
import Button from './ui/Button.vue';
import { RefreshCcw } from '@lucide/vue';

const { isLoading, error, listVaultIds, unlockVault } = useVault();

const vaultIds = ref<string[]>([]);
const selectedVaultId = ref<string | null>(null);
const masterPassword = ref('');

async function loadIds() {
  vaultIds.value = await listVaultIds();
}

async function handleUnlock() {
  if (!selectedVaultId.value || !masterPassword.value) return;

  const success = await unlockVault(selectedVaultId.value, masterPassword.value);

  if (success) {
    masterPassword.value = '';
  }
}
</script>

<template>
  <div class="unlock-vault">
    <h2>Unlock Existing Vault</h2>

    <!-- Show list of vaults -->
    <div v-if="!selectedVaultId">
      <Button
        @click="loadIds"
        variant="outline"
        size="small"
        :icon-component="RefreshCcw"
        :disabled="isLoading"
      >
        Refresh Vault List
      </Button>

      <ul v-if="vaultIds.length > 0" class="vault-list">
        <li v-for="id in vaultIds" :key="id" @click="selectedVaultId = id" class="vault-item">
          {{ id }}
        </li>
      </ul>
      <p v-else-if="!isLoading">No vaults found. Create one first!</p>
    </div>

    <!-- Show password prompt for selected vault -->
    <form v-else @submit.prevent="handleUnlock">
      <p>
        Unlocking: <strong>{{ selectedVaultId }}</strong>
      </p>

      <div>
        <label for="unlock-pw">Master Password:</label>
        <input id="unlock-pw" v-model="masterPassword" type="password" required />
      </div>

      <div class="button-group">
        <Button type="button" @click="selectedVaultId = null">Back</Button>
        <Button type="submit" :disabled="isLoading">
          {{ isLoading ? 'Decrypting...' : 'Unlock' }}
        </Button>
      </div>
    </form>

    <p v-if="error" class="error">{{ error }}</p>
  </div>
</template>

<style scoped>
.unlock-vault {
  max-width: 400px;
  margin: 2rem auto;
  padding: 1rem;
}
.vault-list {
  list-style: none;
  padding: 0;
  margin-top: 1rem;
}
.vault-item {
  padding: 0.75rem;
  border: 1px solid #ccc;
  margin-bottom: 0.5rem;
  cursor: pointer;
  font-family: monospace; /* UUIDs look better in monospace */
  border-radius: 4px;
}
.vault-item:hover {
  background-color: #f0f0f0;
}
input {
  width: 100%;
  padding: 0.5rem;
  margin-top: 0.25rem;
  box-sizing: border-box;
}
.button-group {
  display: flex;
  gap: 1rem;
  margin-top: 1rem;
}
.button-group button {
  flex: 1;
  padding: 0.75rem;
}
.error {
  color: red;
  margin-top: 1rem;
}
</style>
