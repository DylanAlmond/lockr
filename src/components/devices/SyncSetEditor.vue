<script setup lang="ts">
import { computed, markRaw, ref, watch } from 'vue';
import { TriangleAlert, Upload } from '@lucide/vue';
import Button from '../ui/Button.vue';
import AlertModal from '../ui/AlertModal.vue';
import VaultRemovalActions from './VaultRemovalActions.vue';
import { useModal } from '../../composables/useModal';
import { useSync } from '../../composables/useSync';
import { useVault } from '../../composables/useVault';
import { vaultLabel } from '../../util/labels';
import useAppStore from '../../stores/appStore';
import type { SyncSetVault, UnregisteredVault } from '../../types';

const props = defineProps<{
  vaults: SyncSetVault[];
  unregistered: UnregisteredVault[];
}>();

const emit = defineEmits<{
  (e: 'changed'): void;
}>();

const { setSyncSet, setVaultSyncing, adoptVault, error } = useSync();
const { removeLocalVault } = useVault();
const { state, deleteVaultById } = useAppStore();
const { openModal } = useModal();

const selected = ref<Set<string>>(new Set());
const busy = ref(false);
const actionMessage = ref<string | null>(null);
const actionError = ref<string | null>(null);

function initialSelection(): string[] {
  return props.vaults
    .filter((vault) => vault.syncing)
    .map((vault) => vault.vault_id)
    .sort();
}

function currentSelection(): string[] {
  return [...selected.value].sort();
}

watch(
  () => props.vaults.map((vault) => `${vault.vault_id}:${vault.syncing}`).join(','),
  () => {
    selected.value = new Set(props.vaults.filter((vault) => vault.syncing).map((vault) => vault.vault_id));
  },
  { immediate: true }
);

const hasChanges = computed(
  () => initialSelection().join(',') !== currentSelection().join(',')
);

function selectionLabel(vault: SyncSetVault): string {
  const chosen = selected.value.has(vault.vault_id);

  if (chosen && vault.syncing) return 'Syncing';
  if (chosen) return 'Will sync';
  if (vault.syncing) return 'Will stop syncing';
  return 'Not syncing';
}

function selectionOn(vault: SyncSetVault): boolean {
  return selected.value.has(vault.vault_id);
}

function toggle(vaultId: string) {
  const next = new Set(selected.value);
  if (next.has(vaultId)) {
    next.delete(vaultId);
  } else {
    next.add(vaultId);
  }
  selected.value = next;
}

async function saveSelection() {
  busy.value = true;
  actionMessage.value = null;

  const saved = await setSyncSet(currentSelection());
  busy.value = false;

  if (saved) {
    actionMessage.value = 'Sync Set saved on this Device.';
    emit('changed');
  }
}

function confirmStopSyncing(vault: SyncSetVault) {
  openModal(markRaw(AlertModal), {
    title: `Stop syncing “${vaultLabel(vault)}”?`,
    message:
      'This Device keeps the Vault and everything in it. It stops being sent to or requested from your other Devices, so the copies will drift. Turning syncing back on merges that divergence and applies any pending deletion.',
    actionLabel: 'Stop syncing',
    onClose: () => {},
    onConfirm: async () => {
      busy.value = true;
      actionError.value = null;
      const saved = await setVaultSyncing(vault.vault_id, false);
      busy.value = false;

      if (saved) {
        const next = new Set(selected.value);
        next.delete(vault.vault_id);
        selected.value = next;
        actionMessage.value = `Stopped syncing “${vaultLabel(vault)}”. The local copy is unchanged.`;
        emit('changed');
      } else {
        actionError.value = 'This Vault could not be removed from the Sync Set.';
      }
    }
  });
}

function confirmRemoveLocal(vault: SyncSetVault) {
  openModal(markRaw(AlertModal), {
    title: `Remove “${vaultLabel(vault)}” from this Device?`,
    message:
      'This Device’s encrypted copy is removed. Your other Devices keep theirs, and no deletion is recorded. This is not “delete everywhere”.',
    actionLabel: 'Remove local copy',
    confirmationValue: vaultLabel(vault),
    onClose: () => {},
    onConfirm: async () => {
      busy.value = true;
      actionError.value = null;
      const removed = await removeLocalVault(vault.vault_id);
      busy.value = false;

      if (removed) {
        const next = new Set(selected.value);
        next.delete(vault.vault_id);
        selected.value = next;
        state.vaultMutationCount += 1;
        actionMessage.value = `Removed this Device’s copy of “${vaultLabel(vault)}”.`;
        emit('changed');
      } else {
        actionError.value = 'This Device’s copy could not be removed.';
      }
    }
  });
}

function confirmDeleteEverywhere(vault: SyncSetVault) {
  openModal(markRaw(AlertModal), {
    title: `Delete “${vaultLabel(vault)}” everywhere?`,
    message:
      'A deletion is recorded and travels to every Device on the next sync. Other Devices move their copy to trash, and syncing cannot bring it back. This is the destructive action.',
    actionLabel: 'Delete everywhere',
    confirmationValue: vaultLabel(vault),
    onClose: () => {},
    onConfirm: async () => {
      busy.value = true;
      actionError.value = null;
      const deleted = await deleteVaultById(vault.vault_id);
      busy.value = false;

      if (deleted) {
        const next = new Set(selected.value);
        next.delete(vault.vault_id);
        selected.value = next;
        actionMessage.value = `Deleted “${vaultLabel(vault)}” everywhere.`;
        emit('changed');
      } else {
        actionError.value = 'The Vault could not be deleted everywhere.';
      }
    }
  });
}

function confirmAdopt(vault: UnregisteredVault) {
  openModal(markRaw(AlertModal), {
    title: `Adopt “${vault.file_name}”?`,
    message:
      'This Vault file is on this Device but not in your Vault list. Adopting it adds it to every Device and shares its entire contents. Only adopt a file you recognise.',
    actionLabel: 'Adopt and share',
    onClose: () => {},
    onConfirm: async () => {
      busy.value = true;
      actionError.value = null;
      const adopted = await adoptVault(vault.vault_id);
      busy.value = false;

      if (adopted) {
        state.vaultMutationCount += 1;
        actionMessage.value = `Adopted “${vault.file_name}” and added it to the Sync Set.`;
        emit('changed');
      } else {
        actionError.value = 'The Vault file could not be adopted.';
      }
    }
  });
}
</script>

<template>
  <section class="sync-set-editor">
    <header class="section-header">
      <div>
        <h2>Sync Set</h2>
        <p class="section-hint">
          Choose which Vaults this Device carries. The choice belongs to this Device, not the Vault.
        </p>
      </div>
      <Button
        variant="accent"
        size="small"
        :disabled="busy || !hasChanges"
        @click="saveSelection"
      >
        Save Sync Set
      </Button>
    </header>

    <p v-if="error" class="error-text">{{ error }}</p>
    <p v-if="actionError" class="error-text">{{ actionError }}</p>
    <p v-if="actionMessage" class="outcome">{{ actionMessage }}</p>

    <div v-if="vaults.length === 0" class="empty-state">
      No Vaults are registered yet. Create one, or adopt a local Vault file below.
    </div>

    <ul v-else class="vault-selection-list">
      <li v-for="vault in vaults" :key="vault.vault_id" class="vault-check">
        <label>
          <input
            type="checkbox"
            :checked="selected.has(vault.vault_id)"
            :disabled="busy"
            @change="toggle(vault.vault_id)"
          />
          <span class="vault-name">{{ vaultLabel(vault) }}</span>
        </label>
        <span class="sync-indicator" :class="{ 'sync-indicator--off': !selectionOn(vault) }">
          {{ selectionLabel(vault) }}
        </span>
        <span v-if="!vault.syncing && selectionOn(vault)" class="re-enable-warning">
          Re-enabling merges this Vault’s divergence.
        </span>
      </li>
    </ul>

    <section class="removal-section">
      <header>
        <h3>Vault removal actions</h3>
        <p class="section-hint">
          These three actions are deliberately different. Only one of them is destructive.
        </p>
      </header>

      <VaultRemovalActions
        v-for="vault in vaults"
        :key="`removal-${vault.vault_id}`"
        :vault="vault"
        :busy="busy"
        @stop-syncing="confirmStopSyncing(vault)"
        @remove-local="confirmRemoveLocal(vault)"
        @delete-everywhere="confirmDeleteEverywhere(vault)"
      />
    </section>

    <section v-if="unregistered.length > 0" class="unregistered-section">
      <header>
        <h3>
          <TriangleAlert :size="16" aria-hidden="true" />
          Vault files found on this Device
        </h3>
        <p class="section-hint">
          These files are not in your Vault list, so they are ignored. Adopting one shares its
          entire contents with your other Devices.
        </p>
      </header>

      <ul class="unregistered-list">
        <li v-for="vault in unregistered" :key="vault.vault_id">
          <code>{{ vault.file_name }}</code>
          <Button
            variant="outline"
            size="xs"
            :icon-component="Upload"
            :disabled="busy"
            @click="confirmAdopt(vault)"
          >
            Adopt and share
          </Button>
        </li>
      </ul>
    </section>
  </section>
</template>

<style scoped src="./SyncSetEditor.css"></style>
