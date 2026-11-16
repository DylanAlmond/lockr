<script setup lang="ts">
import { computed, ref } from 'vue';
import Button from '../ui/Button.vue';
import { formatConflictValue } from '../../util/conflictRedaction';
import { conflictFieldLabel } from '../../util/conflictLabels';
import { candidateLabel } from '../../util/labels';
import type { DuplicateField, DuplicateSuggestion } from '../../types';

const props = defineProps<{
  suggestion: DuplicateSuggestion;
  busy: boolean;
}>();

const emit = defineEmits<{
  (e: 'apply', survivor: string, fieldChoices: Record<string, string>): void;
  (e: 'cancel'): void;
}>();

const survivor = ref<string | null>(null);
const fieldChoices = ref<Record<string, string>>({});

function fieldValue(field: DuplicateField, itemId: string): string {
  if (field.is_secret) return 'Hidden';
  return itemId === props.suggestion.first.item_id
    ? formatConflictValue(field.first_value)
    : formatConflictValue(field.second_value);
}

function chooseField(fieldKey: string, itemId: string) {
  fieldChoices.value = { ...fieldChoices.value, [fieldKey]: itemId };
}

const canCombine = computed(() => {
  if (!survivor.value) return false;
  return props.suggestion.colliding_fields.every(
    (field) => !!fieldChoices.value[field.field_key]
  );
});

function apply() {
  if (!survivor.value || !canCombine.value) return;
  emit('apply', survivor.value, { ...fieldChoices.value });
}
</script>

<template>
  <div class="combine-panel">
    <h3>Combine these Items</h3>
    <p class="combine-hint">
      Choose the identity that survives, then pick a value for every field that differs. Secret
      values are never shown; a pick chooses which hidden value stays.
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
        {{ conflictFieldLabel(field.field_key) }}
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
      <Button variant="label" size="small" :disabled="busy" @click="emit('cancel')">
        Cancel
      </Button>
      <Button variant="accent" size="small" :disabled="busy || !canCombine" @click="apply">
        Combine Items
      </Button>
    </div>
  </div>
</template>

<style scoped>
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

.secret-chip {
  margin-left: 0.375rem;
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
  background: hsl(38 90% 94%);
  color: hsl(30 60% 28%);
  font-size: 0.6875rem;
}

.combine-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  justify-content: flex-end;
}
</style>
