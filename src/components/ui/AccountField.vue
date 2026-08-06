<script setup lang="ts">
import { ref, computed, onUnmounted } from 'vue';
import { Copy, Check, Eye, EyeOff } from '@lucide/vue';
import Button from './Button.vue';

type CopyableValue = string | null | (() => string | null | Promise<string | null>);

interface Props {
  label: string;
  displayValue?: string | null;
  copyValue?: CopyableValue;
  canCopy?: boolean;
  type?: 'text' | 'email' | 'tel' | 'password';
  modelValue?: string | null;
  input?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  displayValue: null,
  canCopy: false,
  inputType: undefined,
  modelValue: null,
  input: false
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const copied = ref(false);
const showPassword = ref(false);
let cooldownTimer: ReturnType<typeof setTimeout> | null = null;

const computedInputType = computed(() => {
  if (props.type === 'password') {
    return showPassword.value ? 'text' : 'password';
  }
  return props.type;
});

async function handleCopy() {
  let valueToCopy: string | null = null;

  if (!props.displayValue && props.type !== undefined && props.modelValue) {
    valueToCopy = props.modelValue;
  } else if (typeof props.copyValue === 'function') {
    valueToCopy = await props.copyValue();
  } else {
    valueToCopy = props.copyValue ?? props.displayValue;
  }

  if (!valueToCopy) return;

  try {
    await navigator.clipboard.writeText(valueToCopy);
    copied.value = true;

    if (cooldownTimer) clearTimeout(cooldownTimer);

    cooldownTimer = setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch (err) {
    console.error('Failed to copy text:', err);
  }
}

onUnmounted(() => {
  if (cooldownTimer) clearTimeout(cooldownTimer);
});
</script>

<template>
  <div class="account-field">
    <div class="field-meta">
      <h2>{{ label }}</h2>

      <!-- Input Mode -->
      <input
        v-if="input"
        :type="computedInputType"
        class="field-input"
        :value="modelValue"
        placeholder="No Value"
        @input="emit('update:modelValue', ($event.target as HTMLInputElement).value)"
      />

      <!-- Display Mode -->
      <template v-else>
        <span v-if="copied" class="copied">Copied!</span>
        <span v-else>{{
          type === 'password' && !showPassword ? '••••••••••••••••' : displayValue || 'No Value'
        }}</span>
      </template>
    </div>

    <div class="field-actions">
      <slot name="actions" />

      <!-- Password Visibility Toggle -->
      <Button
        v-if="type === 'password'"
        aria-label="Toggle password visibility"
        icon-only
        variant="outline"
        size="small"
        :icon-component="showPassword ? EyeOff : Eye"
        @click="showPassword = !showPassword"
      />

      <Button
        v-if="canCopy"
        :aria-label="`Copy ${label}`"
        icon-only
        variant="outline"
        size="small"
        :icon-component="copied ? Check : Copy"
        @click="handleCopy"
      />
    </div>
  </div>
</template>

<style scoped>
.account-field {
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

.account-field:focus-within {
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

@supports (corner-shape: squircle) {
  .account-field {
    corner-shape: squircle;
  }
}

.copied {
  color: var(--color-green);
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
