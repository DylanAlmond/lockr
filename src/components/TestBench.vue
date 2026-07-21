<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import {
  UserPlus,
  LogIn,
  LogOut,
  RefreshCw,
  Eye,
  KeyRound,
  Trash2,
  Save,
  Search,
  Star
} from '@lucide/vue';
import Button from './ui/Button.vue';
import Input from './ui/Input.vue';
import { useUser } from '../composables/useUser';
import { useVault } from '../composables/useVault';
import type { Account } from '../types';

const { register, login, logout, fetchUser, updateProfile, deleteUser } = useUser();
const {
  unlockedVaults,
  setUnlockedVaults,
  listVaultIds,
  getUnlockedVaults,
  createVault,
  updateVault,
  deleteVault,
  addAccount,
  getAllAccounts,
  getAccount,
  updateAccount,
  deleteAccount,
  getSecret,
  error
} = useVault();

// State
const isRegistered = ref(false);
const log = ref<string[]>(['Waiting for action...']);

// Auth State
const authName = ref('Test User');
const authPassword = ref('SuperSecretPassword123!');

// Vault State
const vaultName = ref('New Vault Name');
const updateVaultId = ref('');
const updateVaultName = ref('');
const updateVaultColor = ref('#ff0000');

// Account State
const accVaultId = ref('');
const accUsername = ref('admin');
const accPassword = ref('hunter2');
const accDisplayName = ref('Admin Account');
const accEmail = ref('admin@test.com');

// Specific Get/Update/Delete State
const targetAccountId = ref('');
const updateFav = ref(false);
const updateTags = ref('social, work');
const updateNewUsername = ref('');
const updateNewPassword = ref('');

// Data State
const accounts = ref<Account[]>([]);
const revealedSecrets = ref<Record<string, string>>({});

// Auto-select vault ID if only one exists
const isAutoSelectVaultDisabled = computed(() => unlockedVaults.value.length !== 1);

onMounted(() => {
  fetchUser().then((u) => {
    if (u) isRegistered.value = true;
  });
});

function addLog(msg: string) {
  const time = new Date().toLocaleTimeString();
  log.value.push(`[${time}] ${msg}`);
}

// ==========================================
// AUTH
// ==========================================
async function handleRegister() {
  addLog('Calling register_user...');
  const vaults = await register(authName.value, authPassword.value);
  if (vaults) {
    setUnlockedVaults(vaults);
    addLog(`SUCCESS! Registered & auto-created ${vaults.length} vault(s).`);
  } else {
    addLog('FAILED to register.');
  }
}

async function handleLogin() {
  addLog('Calling login_user...');
  const vaults = await login(authPassword.value);
  if (vaults) {
    setUnlockedVaults(vaults);
    addLog(`SUCCESS! Logged in & unlocked ${vaults.length} vault(s).`);
  } else {
    addLog('FAILED to login.');
  }
}

async function handleLogout() {
  addLog('Calling logout. Zeroizing RAM...');
  const success = await logout();
  if (success) {
    accounts.value = [];
    revealedSecrets.value = {};
    addLog('SUCCESS! Logged out.');
  }
}

// ==========================================
// USER PROFILE
// ==========================================
async function handleFetchProfile() {
  addLog('Fetching user profile...');
  const user = await fetchUser();
  if (user) {
    authName.value = user.name;
    addLog(`SUCCESS! Profile: ${user.name} (${user.color})`);
  } else {
    addLog('FAILED to fetch profile (Not registered?).');
  }
}

async function handleUpdateProfile() {
  addLog(`Updating profile to name: "${authName.value}"...`);
  const success = await updateProfile({ name: authName.value });
  if (success) addLog('SUCCESS! Profile updated.');
  else addLog('FAILED to update profile.');
}

async function handleDeleteUser() {
  if (!confirm('This will delete user.json and all linked vault files. Continue?')) return;
  addLog('Deleting user entirely...');
  const success = await deleteUser();
  if (success) {
    isRegistered.value = false;
    unlockedVaults.value = [];
    accounts.value = [];
    addLog('SUCCESS! User deleted.');
  } else {
    addLog('FAILED to delete user.');
  }
}

// ==========================================
// VAULTS
// ==========================================
async function handleCreateVault() {
  addLog(`Creating vault "${vaultName.value}"...`);
  const vault = await createVault(vaultName.value);
  if (vault) addLog(`SUCCESS! Created: ${vault.name}`);
  else addLog('FAILED to create vault.');
}

async function handleListDiskVaults() {
  addLog('Listing vault files on disk (includes locked vaults)...');
  const ids = await listVaultIds();
  addLog(`SUCCESS! Found ${ids.length} vault files on disk.`);
}

async function handleGetUnlockedVaults() {
  addLog('Fetching vaults currently in RAM...');
  const vaults = await getUnlockedVaults();
  addLog(`SUCCESS! ${vaults.length} vaults in RAM.`);
}

async function handleUpdateVault() {
  if (!updateVaultId.value) return addLog('ERROR: No vault selected in dropdown.');
  addLog(`Updating vault...`);
  const success = await updateVault(updateVaultId.value, {
    name: updateVaultName.value || null,
    color: updateVaultColor.value || null
  });
  if (success) {
    setUnlockedVaults([...unlockedVaults.value]);
    addLog('SUCCESS! Vault updated.');
  } else {
    addLog('FAILED to update vault.');
  }
}

async function handleDeleteVault() {
  if (!updateVaultId.value) return addLog('ERROR: No vault selected in dropdown.');
  if (!confirm(`Delete selected vault?`)) return;

  addLog(`Deleting vault...`);
  const success = await deleteVault(updateVaultId.value);
  if (success) {
    setUnlockedVaults(unlockedVaults.value.filter((v) => v.id !== updateVaultId.value));
    updateVaultId.value = ''; // Reset dropdown
    addLog('SUCCESS! Vault deleted.');
  } else {
    addLog('FAILED to delete vault.');
  }
}

// ==========================================
// ACCOUNTS
// ==========================================
async function handleAddAccount() {
  const vId = unlockedVaults.value.length === 1 ? unlockedVaults.value[0].id : accVaultId.value;
  if (!vId) return addLog('ERROR: No vault selected.');

  addLog(`Adding account "${accUsername.value}"...`);
  const account = await addAccount(
    vId,
    accUsername.value,
    accPassword.value,
    accDisplayName.value,
    accEmail.value
  );
  if (account) {
    addLog(`SUCCESS! Added: ${account.username}`);
    handleFetchAccounts();
  } else {
    addLog('FAILED to add account.');
  }
}

async function handleFetchAccounts() {
  addLog('Fetching all accounts (flat list)...');
  const list = await getAllAccounts();
  accounts.value = list;
  addLog(`SUCCESS! Fetched ${list.length} accounts.`);
}

async function handleGetSpecificAccount() {
  if (!targetAccountId.value) return addLog('ERROR: No account selected in dropdown.');
  const vId = unlockedVaults.value.length === 1 ? unlockedVaults.value[0].id : accVaultId.value;
  if (!vId) return addLog('ERROR: No vault selected.');

  addLog(`Fetching specific account...`);
  const account = await getAccount(vId, targetAccountId.value);
  if (account) addLog(`SUCCESS! Fetched: ${JSON.stringify(account)}`);
  else addLog('FAILED to fetch account.');
}

async function handleUpdateAccount() {
  if (!targetAccountId.value) return addLog('ERROR: No account selected in dropdown.');
  const vId = unlockedVaults.value.length === 1 ? unlockedVaults.value[0].id : accVaultId.value;
  if (!vId) return addLog('ERROR: No vault selected.');

  const tagsArray = updateTags.value
    .split(',')
    .map((t) => t.trim())
    .filter((t) => t);

  addLog(`Updating account...`);
  const updated = await updateAccount(vId, targetAccountId.value, {
    username: updateNewUsername.value || null,
    password: updateNewPassword.value || null,
    favourite: updateFav.value,
    tags: tagsArray
  });
  if (updated) {
    addLog(`SUCCESS! Updated.`);
    handleFetchAccounts();
  } else {
    addLog('FAILED to update.');
  }
}

async function handleDeleteAccount() {
  if (!targetAccountId.value) return addLog('ERROR: No account selected in dropdown.');
  const vId = unlockedVaults.value.length === 1 ? unlockedVaults.value[0].id : accVaultId.value;
  if (!vId) return addLog('ERROR: No vault selected.');

  addLog(`Deleting account...`);
  const success = await deleteAccount(vId, targetAccountId.value);
  if (success) {
    targetAccountId.value = ''; // Reset dropdown
    addLog('SUCCESS! Deleted.');
    handleFetchAccounts();
  } else {
    addLog('FAILED to delete.');
  }
}

async function handleReveal(vaultId: string, accountId: string) {
  addLog(`Fetching secret...`);
  const secret = await getSecret(vaultId, accountId);
  if (secret) {
    revealedSecrets.value[accountId] = secret;
    addLog(`SUCCESS! Secret revealed.`);
  } else {
    addLog('FAILED to get secret.');
  }
}
</script>

<template>
  <div class="bench-container">
    <h1 class="bench-title">Full Stack Test Bench</h1>
    <p class="bench-subtitle">Tests every Rust command and Composable function</p>

    <!-- Log (Moved to top so it's always visible) -->
    <div class="card log-card">
      <h2>Execution Log</h2>
      <pre class="log-output">{{ log.join('\n') }}{{ error ? '\n\n[ERROR] ' + error : '' }}</pre>
    </div>

    <!-- 1. Auth -->
    <div class="card">
      <h2>1. Authentication & User</h2>
      <div class="form-row" v-if="!isRegistered">
        <Input v-model="authName" placeholder="Your Name" />
      </div>
      <div class="form-row">
        <Input v-model="authPassword" type="password" placeholder="Master Password" />
      </div>
      <div class="action-row">
        <Button v-if="!isRegistered" @click="handleRegister" :iconComponent="UserPlus"
          >Register & Login</Button
        >
        <Button v-else @click="handleLogin" :iconComponent="LogIn">Login</Button>
        <Button variant="label" @click="handleFetchProfile" :iconComponent="RefreshCw"
          >Get Profile</Button
        >
        <Button variant="label" @click="handleUpdateProfile" :iconComponent="Save"
          >Save Profile</Button
        >
        <Button variant="label" @click="handleDeleteUser" :iconComponent="Trash2" style="color: red"
          >Delete User</Button
        >
      </div>
    </div>

    <!-- 2. Vaults -->
    <div class="card">
      <h2>2. Vault Management</h2>
      <div class="form-row">
        <Input v-model="vaultName" placeholder="New Vault Name" />
        <Button @click="handleCreateVault" variant="outline" size="small" :iconComponent="UserPlus"
          >Create</Button
        >
      </div>
      <div class="action-row">
        <Button @click="handleListDiskVaults" variant="label" size="small">List on Disk</Button>
        <Button @click="handleGetUnlockedVaults" variant="label" size="small">List in RAM</Button>
      </div>

      <div class="vault-chips" v-if="unlockedVaults.length > 0">
        <span
          class="chip"
          v-for="v in unlockedVaults"
          :key="v.id"
          :style="{ borderColor: v.color }"
        >
          <KeyRound :size="14" :stroke-width="2" />
          {{ v.name }}
        </span>
      </div>

      <!-- Update/Delete Vault Sub-card -->
      <div class="sub-card">
        <h3>Update / Delete Vault</h3>
        <div class="form-row" v-if="isAutoSelectVaultDisabled">
          <!-- REPLACED INPUT WITH DROPDOWN -->
          <select v-model="updateVaultId" class="select">
            <option value="" disabled>Select Vault</option>
            <option v-for="v in unlockedVaults" :key="v.id" :value="v.id">{{ v.name }}</option>
          </select>
        </div>
        <div class="hint" v-else>Auto-selected: {{ unlockedVaults[0]?.name }}</div>

        <div class="form-row">
          <Input v-model="updateVaultName" placeholder="New Name" />
          <Input v-model="updateVaultColor" placeholder="Hex Color" style="max-width: 120px" />
        </div>
        <div class="action-row">
          <Button @click="handleUpdateVault" variant="label" size="small" :iconComponent="Save"
            >Update</Button
          >
          <Button
            @click="handleDeleteVault"
            variant="label"
            size="small"
            :iconComponent="Trash2"
            style="color: red"
            >Delete</Button
          >
        </div>
      </div>
    </div>

    <!-- 3. Accounts -->
    <div class="card">
      <h2>3. Flat Accounts</h2>

      <!-- Add Account -->
      <div class="sub-card">
        <h3>Add Account</h3>
        <div class="form-row" v-if="isAutoSelectVaultDisabled">
          <!-- REPLACED INPUT WITH DROPDOWN -->
          <select v-model="accVaultId" class="select">
            <option value="" disabled>Select Vault</option>
            <option v-for="v in unlockedVaults" :key="v.id" :value="v.id">{{ v.name }}</option>
          </select>
        </div>
        <div class="hint" v-else>Auto-selected: {{ unlockedVaults[0]?.name }}</div>

        <div class="form-row">
          <Input v-model="accDisplayName" placeholder="Display Name (opt)" />
          <Input v-model="accUsername" placeholder="Username" />
        </div>
        <div class="form-row">
          <Input v-model="accEmail" placeholder="Email (opt)" type="email" />
          <Input v-model="accPassword" type="password" placeholder="Password" />
        </div>
        <Button @click="handleAddAccount" variant="outline" size="small">Add Account</Button>
      </div>

      <!-- List Accounts -->
      <div class="action-row" style="margin-bottom: 1rem">
        <Button @click="handleFetchAccounts" variant="label" size="small" :iconComponent="RefreshCw"
          >Refresh List</Button
        >
      </div>

      <!-- Get/Update/Delete Specific -->
      <div class="sub-card">
        <h3>Get / Update / Delete Account</h3>
        <div class="form-row">
          <!-- REPLACED INPUT WITH DROPDOWN -->
          <select v-model="targetAccountId" class="select">
            <option value="" disabled>Select Account</option>
            <option v-for="acc in accounts" :key="acc.id" :value="acc.id">
              {{ acc.display_name || acc.username }} ({{ acc.username }})
            </option>
          </select>
        </div>

        <div class="form-row">
          <Button
            @click="handleGetSpecificAccount"
            variant="label"
            size="small"
            :iconComponent="Search"
            >Get Raw</Button
          >
          <Button
            @click="handleDeleteAccount"
            variant="label"
            size="small"
            :iconComponent="Trash2"
            style="color: red"
            >Delete</Button
          >
        </div>

        <div class="form-row">
          <Input v-model="updateNewUsername" placeholder="New Username" />
          <Input v-model="updateNewPassword" type="password" placeholder="New Pass (opt)" />
        </div>
        <div class="form-row">
          <label class="checkbox-label">
            <input type="checkbox" v-model="updateFav" /> Mark Favourite
          </label>

          <Input v-model="updateTags" placeholder="Tags (comma sep)" />
        </div>
        <Button @click="handleUpdateAccount" variant="outline" size="small" :iconComponent="Save"
          >Update Account</Button
        >
      </div>
    </div>

    <!-- 4. Results -->
    <div class="card" v-if="accounts.length > 0">
      <h2>4. Data Output</h2>
      <ul class="account-list">
        <li v-for="acc in accounts" :key="acc.id" class="account-item">
          <div class="account-info">
            <strong>{{ acc.display_name || acc.username }}</strong>
            <span class="meta">
              {{ acc.username }} | {{ acc.vault_id.substring(0, 8) }}... |
              <Star
                v-if="acc.favourite"
                :size="12"
                :stroke-width="2"
                style="color: var(--color-accent); display: inline"
              />
            </span>
          </div>
          <div class="account-actions">
            <span v-if="revealedSecrets[acc.id]" class="secret-text">{{
              revealedSecrets[acc.id]
            }}</span>
            <Button
              size="small"
              variant="label"
              @click="handleReveal(acc.vault_id, acc.id)"
              :iconComponent="Eye"
              >Reveal</Button
            >
          </div>
        </li>
      </ul>
    </div>

    <!-- 5. Logout -->
    <div class="card logout-card">
      <Button @click="handleLogout" variant="label" :iconComponent="LogOut"
        >Logout & Wipe RAM</Button
      >
    </div>
  </div>
</template>

<style scoped>
.bench-container {
  max-width: 650px;
  margin: 2rem auto;
  padding: 0 1rem;
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
  font-family: var(--font-ui);
  color: var(--color-text);
}
.bench-title {
  font-family: var(--font-geo);
  font-size: 1.5rem;
}
.bench-subtitle {
  color: var(--color-text-muted);
  margin-bottom: 1rem;
  font-size: 0.9rem;
}
.card {
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
  padding: 1.5rem;
  box-shadow: var(--shadow-sm);
}
@supports (corner-shape: squircle) {
  .card {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}
h2 {
  font-size: 1.1rem;
  margin-bottom: 0.75rem;
  color: var(--color-text-secondary);
}
h3 {
  font-size: 0.95rem;
  margin-bottom: 0.5rem;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}
.hint {
  font-size: 0.85rem;
  color: var(--color-text-muted);
  margin-bottom: 1rem;
}
.form-row {
  margin-bottom: 0.75rem;
  display: flex;
  gap: 0.5rem;
}
.action-row {
  display: flex;
  gap: 0.75rem;
  margin-top: 0.5rem;
  flex-wrap: wrap;
}
.sub-card {
  margin-top: 1.5rem;
  padding-top: 1.5rem;
  border-top: 1px solid var(--color-border);
}
.vault-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  margin-top: 1rem;
}
.chip {
  display: inline-flex;
  align-items: center;
  gap: 0.4rem;
  padding: 0.4rem 0.75rem;
  background: var(--color-hover);
  border: 1px solid var(--color-border);
  border-radius: 100px;
  font-size: 0.8rem;
  font-weight: 500;
}
.account-list {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}
.account-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0.75rem;
  background: var(--color-hover);
  border-radius: 0.5rem;
  flex-wrap: wrap;
  gap: 0.5rem;
}
.account-info {
  display: flex;
  flex-direction: column;
  gap: 0.2rem;
}
.meta {
  font-size: 0.75rem;
  color: var(--color-text-muted);
  font-family: monospace;
  display: flex;
  align-items: center;
  gap: 6px;
}
.account-actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
}
.secret-text {
  font-family: monospace;
  color: var(--color-accent);
  font-size: 0.9rem;
  word-break: break-all;
  max-width: 200px;
}
.logout-card {
  display: flex;
  justify-content: flex-end;
  background: transparent;
  border: none;
  box-shadow: none;
  padding: 0;
}
.log-card {
  background: hsl(0 0 6%);
  border-color: hsl(0 0 15%);
}
.log-card h2 {
  color: var(--color-green);
}
.log-output {
  background: hsl(0 0 3%);
  padding: 1rem;
  border-radius: 0.5rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  font-size: 0.8rem;
  color: var(--color-text-muted);
  white-space: pre-wrap;
  word-break: break-all;
  max-height: 400px;
  overflow-y: auto;
  margin: 0;
}
.checkbox-label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.9rem;
  cursor: pointer;
  min-width: fit-content;
}
.checkbox-label input {
  width: 16px;
  height: 16px;
  accent-color: var(--color-accent);
}
</style>
```
