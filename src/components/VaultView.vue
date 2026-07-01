<script setup lang="ts">
import { ref, computed } from 'vue';
import { useVault } from '../composables/useVault';

const { currentVault, addService, deleteService, addAccount, deleteAccount, lockVault } =
  useVault();

const newServiceName = ref('');
const selectedServiceId = ref<string | null>(null);
const newUsername = ref('');
const newPassword = ref('');

const selectedService = computed(() => {
  if (!selectedServiceId.value) return null;
  return currentVault.value?.services.find((s) => s.id === selectedServiceId.value) ?? null;
});

async function handleAddService() {
  if (!newServiceName.value) return;
  const s = await addService(newServiceName.value);
  if (s) {
    selectedServiceId.value = s.id;
    newServiceName.value = '';
  }
}

async function handleDeleteService(id: string) {
  if (confirm('Delete this service and all its accounts?')) {
    await deleteService(id);
    if (selectedServiceId.value === id) selectedServiceId.value = null;
  }
}

async function handleAddAccount() {
  if (!selectedServiceId.value || !newUsername.value) return;
  const a = await addAccount(selectedServiceId.value, newUsername.value, newPassword.value);
  if (a) {
    newUsername.value = '';
    newPassword.value = '';
  }
}

async function handleDeleteAccount(serviceId: string, accountId: string) {
  await deleteAccount(serviceId, accountId);
}
</script>

<template>
  <div v-if="currentVault" class="vault-view">
    <div class="header">
      <h2>{{ currentVault.name }}</h2>
      <button @click="lockVault" class="lock-btn">Lock Vault</button>
    </div>

    <div class="columns">
      <!-- Services List -->
      <div class="col">
        <h3>Services</h3>
        <form @submit.prevent="handleAddService">
          <input v-model="newServiceName" placeholder="New service name..." required />
          <button type="submit">Add</button>
        </form>
        <ul>
          <li
            v-for="s in currentVault.services"
            :key="s.id"
            @click="selectedServiceId = s.id"
            :class="{ active: selectedServiceId === s.id }"
          >
            {{ s.name }} ({{ s.accounts.length }})
            <button @click.stop="handleDeleteService(s.id)" class="danger">X</button>
          </li>
        </ul>
      </div>

      <!-- Accounts List -->
      <div class="col">
        <h3 v-if="selectedService">Accounts for {{ selectedService.name }}</h3>
        <p v-else>Select a service...</p>

        <form v-if="selectedService" @submit.prevent="handleAddAccount">
          <input v-model="newUsername" placeholder="Username" required />
          <input v-model="newPassword" type="password" placeholder="Password" required />
          <button type="submit">Add Account</button>
        </form>

        <div v-if="selectedService" class="accounts-list">
          <div v-for="a in selectedService.accounts" :key="a.id" class="account-card">
            <strong>{{ a.username }}</strong>
            <button @click="handleDeleteAccount(selectedService.id, a.id)" class="danger sm">
              Delete
            </button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.vault-view {
  padding: 1rem;
  max-width: 800px;
  margin: auto;
}
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-bottom: 2px solid #ccc;
  padding-bottom: 0.5rem;
}
.lock-btn {
  background: #ff4d4f;
  color: white;
  border: none;
  padding: 0.5rem 1rem;
  cursor: pointer;
}
.columns {
  display: flex;
  gap: 2rem;
  margin-top: 1rem;
}
.col {
  flex: 1;
}
form {
  display: flex;
  gap: 0.5rem;
  margin-bottom: 1rem;
}
input {
  flex: 1;
  padding: 0.5rem;
}
ul {
  list-style: none;
  padding: 0;
}
li {
  padding: 0.5rem;
  border: 1px solid #ddd;
  margin-bottom: 0.5rem;
  cursor: pointer;
  display: flex;
  justify-content: space-between;
  border-radius: 4px;
}
li.active {
  background: #e6f7ff;
  border-color: #1890ff;
}
.danger {
  background: #ff4d4f;
  color: white;
  border: none;
  padding: 0.2rem 0.5rem;
  cursor: pointer;
}
.danger.sm {
  font-size: 0.8rem;
}
.account-card {
  display: flex;
  justify-content: space-between;
  padding: 0.5rem;
  border: 1px solid #eee;
  margin-bottom: 0.5rem;
  border-radius: 4px;
}
</style>
