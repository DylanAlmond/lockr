<script setup lang="ts">
import { ref, computed, nextTick } from 'vue';
import { useVault } from '../composables/useVault';
import { Account } from '../types';

const {
  currentVault,
  addService,
  deleteService,
  addAccount,
  updateAccount,
  deleteAccount,
  updateVaultName,
  updateServiceName,
  lockVault,
  getSecret
} = useVault();

// --- Service State ---
const newServiceName = ref('');
const selectedServiceId = ref<string | null>(null);
const editingServiceId = ref<string | null>(null);
const serviceNameInput = ref('');

// --- Account State ---
const newDisplayName = ref('');
const newUsername = ref('');
const newEmail = ref('');
const newPassword = ref('');

const revealedPasswords = ref<Record<string, string>>({});

// --- Edit Account State ---
const editingAccountId = ref<string | null>(null);
const editForm = ref({ displayName: '', username: '', email: '', password: '' });

// --- Vault Name State ---
const editingVaultName = ref(false);
const vaultNameInput = ref('');

const selectedService = computed(() => {
  if (!selectedServiceId.value) return null;
  return currentVault.value?.services.find((s) => s.id === selectedServiceId.value) ?? null;
});

// --- Vault Name Handlers ---
function startEditVaultName() {
  if (!currentVault.value) return;
  vaultNameInput.value = currentVault.value.name;
  editingVaultName.value = true;
  nextTick(() => document.querySelector<HTMLInputElement>('.header .edit-input')?.focus());
}

async function saveVaultName() {
  if (!editingVaultName.value) return;
  editingVaultName.value = false;
  if (currentVault.value && vaultNameInput.value.trim() !== '') {
    await updateVaultName(vaultNameInput.value.trim());
  }
}

// --- Service Handlers ---
async function handleAddService() {
  if (!newServiceName.value) return;
  const s = await addService(newServiceName.value);
  if (s) {
    selectedServiceId.value = s.id;
    newServiceName.value = '';
  }
}

function startEditService(id: string, currentName: string) {
  editingServiceId.value = id;
  serviceNameInput.value = currentName;
  nextTick(() => {
    const input = document.querySelector(`li .edit-input`) as HTMLInputElement | null;
    input?.focus();
  });
}

async function saveServiceName(id: string) {
  editingServiceId.value = null;
  if (serviceNameInput.value.trim() !== '') {
    await updateServiceName(id, serviceNameInput.value.trim());
  }
}

async function handleDeleteService(id: string) {
  if (confirm('Delete this service and all its accounts?')) {
    await deleteService(id);
    if (selectedServiceId.value === id) selectedServiceId.value = null;
  }
}

// --- Account Handlers ---
async function handleAddAccount() {
  if (!selectedServiceId.value || !newUsername.value) return;
  // Passing the optional fields!
  const a = await addAccount(
    selectedServiceId.value,
    newUsername.value,
    newPassword.value,
    newDisplayName.value || null, // Pass null if empty
    newEmail.value || null // Pass null if empty
  );
  if (a) {
    newDisplayName.value = '';
    newUsername.value = '';
    newEmail.value = '';
    newPassword.value = '';
  }
}

function startEditAccount(account: Account) {
  editingAccountId.value = account.id;
  editForm.value = {
    displayName: account.display_name ?? '',
    username: account.username,
    email: account.email ?? '',
    password: '' // Leave blank to indicate "don't change"
  };
}

async function handleUpdateAccount(accountId: string) {
  if (!selectedServiceId.value) return;

  await updateAccount(selectedServiceId.value, accountId, {
    displayName: editForm.value.displayName || null, // Empty string clears it
    username: editForm.value.username,
    email: editForm.value.email || null, // Empty string clears it
    password: editForm.value.password || undefined // Undefined means "don't send to Rust"
  });

  editingAccountId.value = null;
}

async function handleDeleteAccount(serviceId: string, accountId: string) {
  if (confirm('Delete this account?')) {
    await deleteAccount(serviceId, accountId);
  }
}

async function revealPassword(serviceId: string, accountId: string) {
  const pw = await getSecret(serviceId, accountId);
  if (pw !== null) {
    revealedPasswords.value[accountId] = pw;
  }
}

async function copyToClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (err) {
    console.error('Failed to copy: ', err);
  }
}
</script>

<template>
  <div v-if="currentVault" class="vault-view">
    <!-- Vault Name (Inline Edit) -->
    <div class="header">
      <div v-if="!editingVaultName" class="vault-title">
        <h2 @dblclick="startEditVaultName">{{ currentVault.name }}</h2>
        <button @click="startEditVaultName" class="btn-sm">Edit Name</button>
      </div>
      <div v-else class="vault-title">
        <input
          v-model="vaultNameInput"
          @blur="saveVaultName"
          @keyup.enter="saveVaultName"
          ref="vaultNameRef"
          class="edit-input"
        />
      </div>
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
            <!-- Service Name (Inline Edit) -->
            <div v-if="editingServiceId !== s.id" class="service-row">
              <span>{{ s.name }} ({{ s.accounts.length }})</span>
              <div>
                <button @click.stop="startEditService(s.id, s.name)" class="btn-sm">Edit</button>
                <button @click.stop="handleDeleteService(s.id)" class="danger btn-sm">X</button>
              </div>
            </div>
            <div v-else @click.stop class="service-row">
              <input
                v-model="serviceNameInput"
                @blur="saveServiceName(s.id)"
                @keyup.enter="saveServiceName(s.id)"
                class="edit-input"
              />
            </div>
          </li>
        </ul>
      </div>

      <!-- Accounts List -->
      <div class="col">
        <h3 v-if="selectedService">Accounts for {{ selectedService.name }}</h3>
        <p v-else>Select a service...</p>

        <form v-if="selectedService" @submit.prevent="handleAddAccount" class="add-account-form">
          <input v-model="newDisplayName" placeholder="Display Name (optional)" />
          <input v-model="newUsername" placeholder="Username" required />
          <input v-model="newEmail" placeholder="Email (optional)" type="email" />
          <input v-model="newPassword" type="password" placeholder="Password" required />
          <button type="submit">Add Account</button>
        </form>

        <div v-if="selectedService" class="accounts-list">
          <div v-for="a in selectedService.accounts" :key="a.id" class="account-card">
            <!-- Account Display -->
            <div v-if="editingAccountId !== a.id" class="account-info">
              <div>
                <strong v-if="a.display_name">{{ a.display_name }}</strong>
                <span v-if="a.display_name && a.username"> ({{ a.username }})</span>
                <strong v-else>{{ a.username }}</strong>
                <div v-if="a.email" class="email">{{ a.email }}</div>
              </div>
              <div class="account-actions">
                <button @click="revealPassword(selectedService.id, a.id)" class="btn-sm">
                  {{ revealedPasswords[a.id] ? 'Hide' : 'Reveal' }}
                </button>
                <button @click="startEditAccount(a)" class="btn-sm">Edit</button>
              </div>
            </div>

            <!-- The Revealed Password (only shown if it exists in our local map) -->
            <div v-if="revealedPasswords[a.id]" class="revealed-pw">
              <code>{{ revealedPasswords[a.id] }}</code>
              <button @click="copyToClipboard(revealedPasswords[a.id])" class="btn-sm">Copy</button>
            </div>

            <!-- Account Edit Form (keep your existing edit form here) -->
            <!-- ... -->

            <button @click="handleDeleteAccount(selectedService.id, a.id)" class="danger sm">
              Delete Account
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
  max-width: 900px;
  margin: auto;
}
.header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  border-bottom: 2px solid #ccc;
  padding-bottom: 0.5rem;
  margin-bottom: 1rem;
}
.vault-title {
  display: flex;
  align-items: center;
  gap: 1rem;
}
.vault-title h2 {
  margin: 0;
  cursor: pointer;
}
.lock-btn {
  background: #ff4d4f;
  color: white;
  border: none;
  padding: 0.5rem 1rem;
  cursor: pointer;
  border-radius: 4px;
}
.columns {
  display: flex;
  gap: 2rem;
}
.col {
  flex: 1;
}
form {
  display: flex;
  gap: 0.5rem;
  margin-bottom: 1rem;
  flex-wrap: wrap;
}
.add-account-form {
  flex-direction: column;
}
.add-account-form input {
  width: 100%;
  box-sizing: border-box;
}
input {
  padding: 0.5rem;
  border: 1px solid #ccc;
  border-radius: 4px;
}
.edit-input {
  flex: 1;
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
  border-radius: 4px;
}
li.active {
  background: #e6f7ff;
  border-color: #1890ff;
}
.service-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
}
.btn-sm {
  background: #f0f0f0;
  border: 1px solid #d9d9d9;
  padding: 0.1rem 0.5rem;
  cursor: pointer;
  border-radius: 3px;
  font-size: 0.85rem;
}
.danger {
  background: #ff4d4f;
  color: white;
  border: none;
  padding: 0.2rem 0.5rem;
  cursor: pointer;
  border-radius: 3px;
}
.danger.sm {
  font-size: 0.8rem;
}
.accounts-list {
  margin-top: 1rem;
}
.account-card {
  display: flex;
  flex-direction: column;
  padding: 0.75rem;
  border: 1px solid #eee;
  margin-bottom: 0.75rem;
  border-radius: 4px;
  background: #fafafa;
}
.account-info {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
}
.email {
  font-size: 0.85rem;
  color: #666;
}
.edit-account-form {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  width: 100%;
}
.edit-account-form input {
  width: 100%;
  box-sizing: border-box;
}
.edit-actions {
  display: flex;
  gap: 0.5rem;
}
.edit-actions button {
  flex: 1;
  padding: 0.5rem;
  cursor: pointer;
  border-radius: 4px;
  border: 1px solid #ccc;
}
.account-actions {
  display: flex;
  gap: 0.5rem;
}
.revealed-pw {
  margin: 0.5rem 0;
  padding: 0.5rem;
  background: #e0e0e0;
  border-radius: 4px;
  display: flex;
  justify-content: space-between;
  align-items: center;
  word-break: break-all;
}
code {
  font-family: monospace;
}
</style>
