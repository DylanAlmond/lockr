<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { RefreshCw, Sparkles } from '@lucide/vue';
import Button from '../ui/Button.vue';
import Select from '../ui/Select.vue';
import { useDuplicates } from '../../composables/useDuplicates';
import type {
  DuplicateCandidate,
  DuplicateField,
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
const survivor = ref<string | null>(null);
const fieldChoices = ref<Record<string, string>>({});

const vaultOptions = computed(() =>
  props.vaults.map((vault) => ({
    value: vault.vault_id,
    label: vault.name?.trim() || `Vault ${vault.vault_id.slice(0, 8)}`
  }))
);

const selectedVault = computed(
  () => props.vaults.find((vault) => vault.vault_id === selectedVaultId.value) ?? null
);

function vaultLabel(vaultId: string): string {
  const vault = props.vaults.find((candidate) => candidate.vault_id === vaultId);
  return vault?.name?.trim() || `Vault ${vaultId.slice(0, 8)}`;
}

function candidateLabel(candidate: DuplicateCandidate): string {
  return candidate.display_name?.trim() || `Item ${candidate.item_id.slice(0, 8)}`;
}

function fieldLabel(fieldKey: string): string {
  return fieldKey
    .replace(/_/g, ' ')
    .replace(/^./, (letter) => letter.toUpperCase());
}

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return 'Not set';
  if (typeof value === 'string') return value.length > 0 ? value : 'Empty';
  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  if (typeof value === 'number') return String(value);
  if (Array.isArray(value)) return value.length > 0 ? value.join(', ') : 'None';
  return JSON.stringify(value);
}

function fieldValue(field: DuplicateField, itemId: string): string {
  if (field.is_secret) return 'Hidden';
  return itemId === combineTarget.value?.first.item_id
    ? formatValue(field.first_value)
    : formatValue(field.second_value);
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
  survivor.value = null;
  fieldChoices.value = {};
  outcome.value = null;
}

function cancelCombine() {
  combineTarget.value = null;
  survivor.value = null;
  fieldChoices.value = {};
}

function chooseField(fieldKey: string, itemId: string) {
  fieldChoices.value = { ...fieldChoices.value, [fieldKey]: itemId };
}

const canCombine = computed(() => {
  const target = combineTarget.value;
  if (!target || !survivor.value) return false;
  return target.colliding_fields.every((field) => !!fieldChoices.value[field.field_key]);
});

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

async function applyCombine() {
  const target = combineTarget.value;
  if (!target || !survivor.value || !canCombine.value) return;

  await applyResolution(
    target,
    {
      resolution: 'combine',
      survivor: survivor.value,
      field_choices: { ...fieldChoices.value }
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
          <strong>Same Item found in {{ note.vault_ids.length }} Vaults</strong>
          <p>
            {{ note.vault_ids.map(vaultLabel).join(' and ') }} — review it as a Duplicate so one
            identity can be retired.
          </p>
        </div>
        <Button variant="outline" size="xs" @click="reviewNote(note)">Review</Button>
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

        <div
          v-if="combineTarget?.first.item_id === suggestion.first.item_id && combineTarget?.second.item_id === suggestion.second.item_id"
          class="combine-panel"
        >
          <h3>Combine these Items</h3>
          <p class="combine-hint">
            Choose the identity that survives, then pick a value for every field that differs.
            Secret values are never shown; a pick chooses which hidden value stays.
          </p>

          <fieldset class="combine-step">
            <legend>Surviving Item</legend>
            <div class="choice-row">
              <button
                v-for="candidate in [suggestion.first, suggestion.second]"
                :key="candidate.item_id"
                type="button"
                class="choice-button"
                :class="{ 'choice-button--selected': survivor === candidate.item_id }"
                :aria-pressed="survivor === candidate.item_id"
                @click="survivor = candidate.item_id"
              >
                {{ candidateLabel(candidate) }}
              </button>
            </div>
          </fieldset>

          <fieldset
            v-for="field in suggestion.colliding_fields"
            :key="field.field_key"
            class="combine-step"
          >
            <legend>
              {{ fieldLabel(field.field_key) }}
              <span v-if="field.is_secret" class="secret-chip">secret</span>
            </legend>

            <div class="choice-row">
              <button
                v-for="candidate in [suggestion.first, suggestion.second]"
                :key="candidate.item_id"
                type="button"
                class="choice-button choice-button--field"
                :class="{
                  'choice-button--selected': fieldChoices[field.field_key] === candidate.item_id
                }"
                :aria-pressed="fieldChoices[field.field_key] === candidate.item_id"
                @click="chooseField(field.field_key, candidate.item_id)"
              >
                <span class="choice-source">{{ candidateLabel(candidate) }}</span>
                <span class="choice-value">{{ fieldValue(field, candidate.item_id) }}</span>
              </button>
            </div>
          </fieldset>

          <div class="combine-actions">
            <Button variant="label" size="small" :disabled="busy" @click="cancelCombine">
              Cancel
            </Button>
            <Button variant="accent" size="small" :disabled="busy || !canCombine" @click="applyCombine">
              Combine Items
            </Button>
          </div>
        </div>
      </article>
    </template>
  </section>
</template>

<style scoped>
.duplicate-review {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.section-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;

  h2 {
    font-family: var(--font-geo);
    font-size: 1.125rem;
    font-weight: 500;
  }
}

.section-hint {
  margin-top: 0.25rem;
  color: var(--color-text-muted);
  font-size: 0.8125rem;
  line-height: 1.4;
}

.notes {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
}

.note-card {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  padding: 0.75rem;
  border-radius: 0.5rem;
  background: var(--color-accent-hover);
  border: 1px solid hsl(256 60% 88%);

  strong {
    font-size: 0.875rem;
  }

  p {
    margin-top: 0.25rem;
    color: var(--color-text-secondary);
    font-size: 0.8125rem;
    line-height: 1.4;
  }
}

.vault-picker {
  display: flex;
  align-items: center;
  gap: 0.75rem;

  label {
    font-size: 0.8125rem;
    color: var(--color-text-muted);
  }
}

.suggestion-card {
  display: flex;
  flex-direction: column;
  gap: 0.875rem;
  padding: 1rem;
  border: 1px solid var(--color-border);
  border-radius: 0.625rem;
}

.suggestion-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  flex-wrap: wrap;
}

.suggestion-names {
  display: flex;
  align-items: baseline;
  gap: 0.5rem;
  font-size: 1rem;
  font-weight: 500;
}

.versus {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
  font-weight: 400;
}

.username {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
}

.collision-summary {
  display: flex;
  align-items: center;
  gap: 0.375rem;
  flex-wrap: wrap;
}

.collision-label {
  color: var(--color-text-muted);
  font-size: 0.75rem;
}

.chip,
.secret-chip {
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
  background: var(--color-hover);
  color: var(--color-text-secondary);
  font-size: 0.6875rem;
}

.secret-chip {
  margin-left: 0.375rem;
  background: hsl(38 90% 94%);
  color: hsl(30 60% 28%);
}

.suggestion-actions,
.combine-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.combine-actions {
  justify-content: flex-end;
}

.combine-panel {
  display: flex;
  flex-direction: column;
  gap: 0.875rem;
  margin-top: 0.25rem;
  padding: 1rem;
  border-top: 1px solid var(--color-border);
  background: var(--color-bg-nav);
  border-radius: 0.5rem;

  h3 {
    font-family: var(--font-geo);
    font-size: 1rem;
    font-weight: 500;
  }
}

.combine-hint {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
  line-height: 1.45;
}

.combine-step {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  border: none;

  legend {
    padding-bottom: 0.375rem;
    font-size: 0.8125rem;
    font-weight: 500;
  }
}

.choice-row {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
  gap: 0.5rem;
}

.choice-button {
  display: flex;
  flex-direction: column;
  gap: 0.25rem;
  padding: 0.625rem;
  text-align: left;
  cursor: pointer;
  background: var(--color-bg);
  color: var(--color-text);
  border: 1px solid var(--color-border);
  border-radius: 0.5rem;
  font: inherit;
  font-size: 0.8125rem;
  transition: all 0.15s ease;

  &:hover {
    border-color: var(--color-accent);
  }

  &:focus-visible {
    outline: none;
    box-shadow: 0 0 0 2px var(--color-accent);
  }
}

.choice-button--selected {
  border-color: var(--color-accent);
  background: var(--color-accent-hover);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}

.choice-source {
  font-weight: 500;
}

.choice-value {
  color: var(--color-text-muted);
  overflow-wrap: anywhere;
}

.empty-state {
  padding: 1rem;
  border: 1px dashed var(--color-border);
  border-radius: 0.5rem;
  color: var(--color-text-muted);
  font-size: 0.875rem;
  line-height: 1.5;
}

.muted {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
}

.error-text {
  color: var(--color-red);
  font-size: 0.8125rem;
}

.outcome {
  color: var(--color-green);
  font-size: 0.8125rem;
}
</style>
