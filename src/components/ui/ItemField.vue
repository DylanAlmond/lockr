<script setup lang="ts">
import { computed } from 'vue';
import { Copy } from '@lucide/vue';
import Button from './Button.vue';
import { ACCOUNT_PASSWORD_MASK } from '../../util/secrets.ts';

type CopyableValue = string | null | (() => string | null | Promise<string | null>);

interface Props {
  id?: string;
  label: string;
  displayValue?: string | null;
  copyValue?: CopyableValue;
  /** Copy through the core (secrets): true on success, false on failure. */
  copyAction?: () => Promise<boolean>;
  canCopy?: boolean;
  type?: 'text' | 'email' | 'tel' | 'password';
  modelValue?: string | null;
  input?: boolean;
  required?: boolean;
  placeholder?: string;
}

const props = withDefaults(defineProps<Props>(), {
  displayValue: null,
  canCopy: false,
  type: 'text',
  modelValue: null,
  input: false,
  required: false,
  placeholder: ''
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
  (e: 'focus'): void;
  (e: 'blur'): void;
}>();

const hasCopyValue = computed(() => {
  if (props.copyAction) return true;
  if (props.input) return !!props.modelValue;
  if (typeof props.copyValue === 'function') return true;
  if (props.copyValue !== undefined && props.copyValue !== null) return !!props.copyValue;
  return !!props.displayValue && props.displayValue !== ACCOUNT_PASSWORD_MASK;
});

async function handleCopy() {
  if (props.copyAction) {
    // Secrets are copied by the core; the value never enters this component.
    if (!(await props.copyAction())) return;
  } else {
    let valueToCopy: string | null = null;

    if (props.input && props.modelValue) {
      valueToCopy = props.modelValue;
    } else if (typeof props.copyValue === 'function') {
      valueToCopy = await props.copyValue();
    } else if (props.copyValue !== undefined && props.copyValue !== null) {
      valueToCopy = props.copyValue;
    } else {
      valueToCopy = props.displayValue;
    }

    if (!valueToCopy || valueToCopy === ACCOUNT_PASSWORD_MASK) return;

    try {
      await navigator.clipboard.writeText(valueToCopy);
    } catch (err) {
      console.error('Failed to copy text:', err);
    }
  }
}
</script>

<template>
  <div :id="id" class="item-field">
    <div class="field-meta">
      <h2>{{ label }}</h2>

      <!-- Input Mode -->
      <input
        v-if="input"
        :type="type"
        class="field-input"
        :value="modelValue || ''"
        :placeholder="placeholder"
        :required="required"
        @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
        @focus="emit('focus')"
        @blur="emit('blur')"
      />

      <!-- Display Mode -->
      <template v-else>
        <span>{{ displayValue || 'No Value' }}</span>
      </template>
    </div>

    <div class="field-actions">
      <!-- Slot for custom actions like Eye/EyeOff or Password Strength -->
      <slot name="actions" />

      <Button
        v-if="canCopy"
        :aria-label="`Copy ${label}`"
        icon-only
        variant="outline"
        size="small"
        :disabled="!hasCopyValue"
        :icon-component="Copy"
        @click="handleCopy"
      />
    </div>
  </div>
</template>

<style scoped>
.item-field {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 1rem 1.5rem;
  overflow: hidden;
  border: 1px solid var(--color-border);
  transition:
    box-shadow 0.2s ease,
    border-color 0.2s ease;
}

.item-field:focus-within {
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

@supports (corner-shape: squircle) {
  .item-field {
    corner-shape: squircle;
  }
}

.field-meta {
  flex: 1;
  min-width: 0;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }

  > span {
    display: block;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
}

.field-actions {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-shrink: 0;
}

.field-input {
  display: block;
  width: 100%;
  font: inherit;
  color: inherit;
  background: transparent;
  border: none;
  outline: none;
  padding: 0;
  margin: 0;
}

.field-input::-ms-reveal {
  display: none;
}

.field-input::placeholder {
  color: var(--color-text-muted);
  opacity: 1;
}

.field-input:disabled {
  cursor: not-allowed;
}

.field-input:-webkit-autofill,
.field-input:-webkit-autofill:hover,
.field-input:-webkit-autofill:focus {
  -webkit-text-fill-color: var(--color-text-tertiary);
  transition: background-color 9999s;
}
</style>
