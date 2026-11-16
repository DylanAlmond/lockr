<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { RefreshCw, Sparkles } from '@lucide/vue';
import Button from '../ui/Button.vue';
import Select from '../ui/Select.vue';
import DuplicateCombinePanel from './DuplicateCombinePanel.vue';
import { useDuplicates } from '../../composables/useDuplicates';
import { vaultLabelById } from '../../util/labels';
import type {
  DuplicateCandidate,
  DuplicateIdentityNote,
  DuplicateResolution,
  DuplicateSuggestion,
  SyncSetVault
} from '../../types';

const props = defineProps<{
  vaults: SyncSetVault[];
  notes: DuplicateIdentityNote[];
}>();

const { suggestions, isLoading, error, listSuggestions, resolve } = useDuplicates();

const selectedVaultId = ref<string | null>(null);
const busy = ref(false);
const outcome = ref<string | null>(null);

const combineTarget = ref<DuplicateSuggestion | null>(null);
const combineSession = ref(0);

const vaultNames = computed<Record<string, string>>(() => {
  const names: Record<string, string> = {};
  for (const vault of props.vaults) {
    if (vault.name?.trim()) names[vault.vault_id] = vault.name.trim();
  }
  return names;
});

const vaultOptions = computed(() =>
  props.vaults.map((vault) => ({
    value: vault.vault_id,
    label: vaultLabelById(vault.vault_id, vaultNames.value)
  }))
);

const selectedVault = computed(
  () => props.vaults.find((vault) => vault.vault_id === selectedVaultId.value) ?? null
);

function vaultName(vaultId: string): string {
  return vaultLabelById(vaultId, vaultNames.value);
}

function candidateLabel(candidate: DuplicateCandidate): string {
  return candidate.display_name?.trim() || `Item ${candidate.item_id.slice(0, 8)}`;
}

function fieldLabel(fieldKey: string): string {
  return fieldKey
    .replace(/_/g, ' ')
    .replace(/^./, (letter) => letter.toUpperCase());
}

async function selectVault(value: string | number) {
  selectedVaultId.value = String(value);
}

async function load() {
  combineTarget.value = null;
  outcome.value = null;
  if (!selectedVaultId.value) {
    suggestions.value = [];
    return;
  }
  await listSuggestions(selectedVaultId.value);
}

function refresh() {
  if (selectedVaultId.value) {
    listSuggestions(selectedVaultId.value);
  }
}

defineExpose({ refresh });

watch(
  () => props.vaults.map((vault) => vault.vault_id).join(','),
  () => {
    if (
      !selectedVaultId.value ||
      !props.vaults.some((vault) => vault.vault_id === selectedVaultId.value)
    ) {
      selectedVaultId.value = props.vaults[0]?.vault_id ?? null;
    }
  },
  { immediate: true }
);

watch(selectedVaultId, load, { immediate: true });

function startCombine(suggestion: DuplicateSuggestion) {
  combineTarget.value = suggestion;
  combineSession.value += 1;
  outcome.value = null;
}

function cancelCombine() {
  combineTarget.value = null;
}

function isCombineTarget(suggestion: DuplicateSuggestion): boolean {
  return (
    combineTarget.value?.first.item_id === suggestion.first.item_id &&
    combineTarget.value?.second.item_id === suggestion.second.item_id
  );
}

async function applyResolution(
  suggestion: DuplicateSuggestion,
  resolution: DuplicateResolution,
  message: string
) {
  if (!selectedVaultId.value) return;

  busy.value = true;
  outcome.value = null;

  const result = await resolve(selectedVaultId.value, suggestion, resolution);
  busy.value = false;

  if (result) {
    outcome.value = message;
    cancelCombine();
  }
}

function keepBoth(suggestion: DuplicateSuggestion) {
  void applyResolution(suggestion, { resolution: 'keep_both' }, 'Kept both Items. This pair will not be suggested again.');
}

function keepOne(suggestion: DuplicateSuggestion, itemId: string) {
  void applyResolution(
    suggestion,
    { resolution: 'keep', item_id: itemId },
    `Kept ${itemId === suggestion.first.item_id ? candidateLabel(suggestion.first) : candidateLabel(suggestion.second)}. The other identity was retired.`
  );
}

async function applyCombine(survivor: string, fieldChoices: Record<string, string>) {
  const target = combineTarget.value;
  if (!target) return;

  await applyResolution(
    target,
    {
      resolution: 'combine',
      survivor,
      field_choices: { ...fieldChoices }
    },
    'Combined the two Items into one identity with your field choices.'
  );
}

function reviewNote(note: DuplicateIdentityNote) {
  const firstVault = note.vault_ids[0];
  if (firstVault && props.vaults.some((vault) => vault.vault_id === firstVault)) {
    selectedVaultId.value = firstVault;
  }
}

function noteTitle(note: DuplicateIdentityNote): string {
  switch (note.reason) {
    case 'cycle':
      return 'Move history forms a loop';
    case 'destination_outside_session':
      return 'Moved to a Vault outside this sync';
    default:
      return `Same Item found in ${note.vault_ids.length} Vaults`;
  }
}

function noteDetail(note: DuplicateIdentityNote): string {
  switch (note.reason) {
    case 'cycle':
      return `This Item's move records point back to ${note.destinations.map(vaultName).join(' and ')}, so no single copy could be found.`;
    case 'destination_outside_session':
      return `A move points at ${note.destinations.map(vaultName).join(' and ')}, which this sync did not include. Sync with a Device that shares that Vault to review the Item.`;
    default:
      return `${note.vault_ids.map(vaultName).join(' and ')} hold live copies. Review it as a Duplicate so one identity can be retired.`;
  }
}
</script>

<template>
  <section class="duplicate-review">
    <header class="section-header">
      <div>
        <h2>Duplicate review</h2>
        <p class="section-hint">
          Suggestions only. Nothing is combined unless you choose every field explicitly.
        </p>
      </div>
      <Button
        variant="outline"
        size="small"
        :icon-component="RefreshCw"
        :disabled="isLoading || !selectedVaultId"
        @click="refresh"
      >
        Refresh
      </Button>
    </header>

    <div v-if="notes.length > 0" class="notes">
      <article v-for="note in notes" :key="note.item_id" class="note-card">
        <div>
          <strong>{{ noteTitle(note) }}</strong>
          <p>{{ noteDetail(note) }}</p>
        </div>
        <Button
          v-if="note.vault_ids.length > 0"
          variant="outline"
          size="xs"
          @click="reviewNote(note)"
        >
          Review
        </Button>
      </article>
    </div>

    <div v-if="vaults.length === 0" class="empty-state">
      <p>No Vaults are registered on this Device yet.</p>
    </div>

    <template v-else>
      <div class="vault-picker">
        <label for="duplicate-vault">Vault</label>
        <Select
          id="duplicate-vault"
          :model-value="selectedVaultId"
          :options="vaultOptions"
          @update:model-value="selectVault"
        />
      </div>

      <p v-if="error" class="error-text">{{ error }}</p>
      <p v-if="outcome" class="outcome">{{ outcome }}</p>

      <p v-if="isLoading" class="muted">Looking for Duplicates…</p>

      <p v-else-if="suggestions.length === 0" class="empty-state">
        No suggested Duplicates in {{ selectedVault?.name ?? 'this Vault' }}. Lockr only suggests
        pairs with the same kind and matching names.
      </p>

      <article
        v-for="suggestion in suggestions"
        :key="`${suggestion.first.item_id}:${suggestion.second.item_id}`"
        class="suggestion-card"
      >
        <header class="suggestion-header">
          <div class="suggestion-names">
            <span>{{ candidateLabel(suggestion.first) }}</span>
            <span class="versus">and</span>
            <span>{{ candidateLabel(suggestion.second) }}</span>
          </div>
          <span v-if="suggestion.username" class="username">
            @{{ suggestion.username }}
          </span>
        </header>

        <div v-if="suggestion.colliding_fields.length > 0" class="collision-summary">
          <span class="collision-label">Fields that differ:</span>
          <span v-for="field in suggestion.colliding_fields" :key="field.field_key" class="chip">
            {{ fieldLabel(field.field_key) }}{{ field.is_secret ? ' (secret)' : '' }}
          </span>
        </div>
        <p v-else class="muted">
          No fields collide. A Combine would keep the surviving Item's values as they are.
        </p>

        <div class="suggestion-actions">
          <Button variant="outline" size="small" :disabled="busy" @click="keepBoth(suggestion)">
            Keep both
          </Button>
          <Button
            variant="outline"
            size="small"
            :disabled="busy"
            @click="keepOne(suggestion, suggestion.first.item_id)"
          >
            Keep {{ candidateLabel(suggestion.first) }}
          </Button>
          <Button
            variant="outline"
            size="small"
            :disabled="busy"
            @click="keepOne(suggestion, suggestion.second.item_id)"
          >
            Keep {{ candidateLabel(suggestion.second) }}
          </Button>
          <Button
            variant="accent"
            size="small"
            :icon-component="Sparkles"
            :disabled="busy"
            @click="startCombine(suggestion)"
          >
            Combine…
          </Button>
        </div>

        <DuplicateCombinePanel
          v-if="isCombineTarget(suggestion)"
          :key="combineSession"
          :suggestion="suggestion"
          :busy="busy"
          @apply="applyCombine"
          @cancel="cancelCombine"
        />
      </article>
    </template>
  </section>
</template>

<style scoped src="./DuplicateReview.css"></style>
