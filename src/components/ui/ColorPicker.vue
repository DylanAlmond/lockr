<script setup lang="ts">
import { useId } from 'vue';
import { VAULT_COLORS } from '../../util/constants.ts';

interface ColorOption {
  hex: string;
  name: string;
}

interface Props {
  modelValue: string;
  colors?: ColorOption[];
  label?: string;
  disabled?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  colors: () => VAULT_COLORS,
  label: 'color',
  disabled: false
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const labelId = useId();
</script>

<template>
  <div class="color-picker">
    <h3 :id="labelId">{{ label }}</h3>
    <div class="color-swatches" role="radiogroup" :aria-labelledby="labelId">
      <button
        v-for="color in props.colors"
        :key="color.hex"
        type="button"
        class="color-swatch"
        role="radio"
        :disabled="disabled"
        :class="{ active: modelValue === color.hex }"
        :style="{ backgroundColor: color.hex, color: color.hex }"
        :aria-checked="modelValue === color.hex"
        :aria-label="color.name"
        @click="emit('update:modelValue', color.hex)"
      />
    </div>
  </div>
</template>

<style scoped>
.color-picker {
  display: flex;
  flex-direction: column;

  > h3 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }
}

.color-swatches {
  display: flex;
  flex-wrap: wrap;
  gap: 0.75rem;
  padding: 0.375rem 0rem;
}

.color-swatch {
  appearance: none;
  border: none;
  outline: none;
  width: 2.25rem;
  height: 2.25rem;
  border-radius: 0.5rem;
  cursor: pointer;
  box-shadow: var(--shadow-sm);
  transition:
    transform 0.15s ease,
    box-shadow 0.2s ease;

  &:hover {
    transform: scale(1.1);
  }

  &:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  &.active {
    box-shadow:
      0 0 0 2px var(--color-bg),
      0 0 0 4px currentColor;
  }

  &:focus-visible {
    box-shadow:
      0 0 0 2px var(--color-bg),
      0 0 0 4px var(--color-accent);
  }

  @supports (corner-shape: squircle) {
    border-radius: 1.5rem;
    corner-shape: squircle;
  }
}
</style>
