<script setup lang="ts">
import { ref } from 'vue';
import { useVault } from '../composables/useVault';
import Button from './ui/Button.vue';
import { Hash, KeyRound, Link2, Phone, Plus, Search } from '@lucide/vue';
import Input from './ui/Input.vue';

const { currentVault, isLoading, error, createVault } = useVault();

const name = ref('');
const masterPassword = ref('');

async function handleSubmit() {
  if (!name.value || !masterPassword.value) return;

  const success = await createVault(name.value, masterPassword.value);

  if (success) {
    // Clear form on success
    name.value = '';
    masterPassword.value = '';
  }
}
</script>

<template>
  <div class="vault-creator">
    <h2>Create New Vault</h2>

    <form @submit.prevent="handleSubmit">
      <div>
        <Input
          :icon-component="Search"
          id="name"
          v-model="name"
          type="text"
          placeholder="e.g., Personal Vaults"
          required
        />
      </div>

      <div>
        <Input
          :icon-component="KeyRound"
          id="password"
          v-model="masterPassword"
          type="password"
          placeholder="Strong password..."
          required
        />
      </div>

      <Button variant="accent" :icon-component="Plus" type="submit" :disabled="isLoading">
        {{ isLoading ? 'Creating...' : 'Create' }}
      </Button>
    </form>

    <!-- Error display -->
    <p v-if="error" class="error">{{ error }}</p>

    <!-- Success display -->
    <div v-if="currentVault" class="success">
      <p>Vault Created! ID: {{ currentVault.id }}</p>
    </div>
  </div>
</template>

<style scoped>
.vault-creator {
  max-width: 400px;
  margin: 2rem auto;
  padding: 1rem;
}
div {
  margin-bottom: 1rem;
}
input {
  width: 100%;
  padding: 0.5rem;
  margin-top: 0.25rem;
  box-sizing: border-box;
}
button {
  width: 100%;
  padding: 0.75rem;
  cursor: pointer;
}
.error {
  color: red;
}
.success {
  color: green;
  margin-top: 1rem;
  padding: 1rem;
  border: 1px solid green;
}
</style>
