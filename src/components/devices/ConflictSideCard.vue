<script setup lang="ts">
import { computed } from 'vue';
import Button from '../ui/Button.vue';
import type { ConflictChoice } from '../../types';
import {
  displayConflictSideValue,
  type ConflictSideEntry
} from '../../util/conflictRedaction';

const props = defineProps<{
  entry: ConflictSideEntry;
  isSecret: boolean;
  revealed: boolean;
  revealedValue: string | null | undefined;
  busy: boolean;
  revealing: boolean;
}>();

const emit = defineEmits<{
  (e: 'reveal'): void;
  (e: 'choose', choice: ConflictChoice): void;
}>();

const value = computed(() =>
  displayConflictSideValue({
    isSecret: props.isSecret,
    reportedValue: props.entry.side.value,
    revealed: props.revealed,
    revealedValue: props.revealedValue
  })
);
</script>

<template>
  <section class="conflict-side" :class="{ 'conflict-side--local': entry.side.is_local }">
    <header>
      <span class="side-device">{{ entry.label }}</span>
      <span v-if="entry.side.is_local" class="side-tag">This copy</span>
    </header>

    <p
      class="side-value"
      :class="{
        'side-value--secret': isSecret && !revealed,
        'side-value--revealed': isSecret && revealed
      }"
    >
      {{ value }}
    </p>

    <Button
      v-if="isSecret && entry.choice"
      variant="label"
      size="small"
      :disabled="busy || revealing"
      @click="emit('reveal')"
    >
      {{ revealing ? 'Revealing…' : revealed ? 'Hide value' : 'Reveal value' }}
    </Button>

    <Button
      v-if="entry.choice"
      :variant="entry.choice === 'local' ? 'accent' : 'outline'"
      size="small"
      :disabled="busy"
      @click="emit('choose', entry.choice)"
    >
      {{ isSecret ? 'Keep this secret value' : 'Keep this value' }}
    </Button>
  </section>
</template>

<style scoped>
.conflict-side {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.875rem;
  border: 1px solid var(--color-border);
  border-radius: 0.5rem;

  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
}

.conflict-side--local {
  border-color: var(--color-accent);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}

.side-device {
  font-size: 0.8125rem;
  font-weight: 500;
}

.side-tag {
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
  background: var(--color-accent-hover);
  color: var(--color-accent);
  font-size: 0.6875rem;
}

.side-value {
  flex: 1;
  width: 100%;
  min-height: 3rem;
  padding: 0.625rem;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
  background: var(--color-bg-nav);
  border-radius: 0.375rem;
  font-size: 0.875rem;
  line-height: 1.45;
}

.side-value--secret {
  display: flex;
  align-items: center;
  color: var(--color-text-muted);
  font-style: italic;
}

.side-value--revealed {
  font-family: ui-monospace, monospace;
  font-size: 0.8125rem;
}
</style>
