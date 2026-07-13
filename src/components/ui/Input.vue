<script setup lang="ts">
import { Eye, EyeOff } from '@lucide/vue';
import { computed, ref, type Component } from 'vue';

interface Props {
  modelValue?: string;
  type?: 'text' | 'email' | 'password' | 'search' | 'url' | 'tel' | 'number';
  placeholder?: string;
  disabled?: boolean;
  name?: string;
  id?: string;
  autocomplete?: string;
  iconComponent?: Component;
}

const props = withDefaults(defineProps<Props>(), {
  modelValue: '',
  type: 'text',
  placeholder: '',
  disabled: false
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const showPassword = ref(false);

const inputType = computed(() => {
  if (props.type !== 'password') {
    return props.type;
  }

  return showPassword.value ? 'text' : 'password';
});

const localValue = computed({
  get: () => props.modelValue,
  set: (value: string) => emit('update:modelValue', value)
});

const hasPasswordToggle = computed(() => props.type === 'password');

function togglePassword() {
  showPassword.value = !showPassword.value;
}
</script>

<template>
  <div class="input">
    <component
      v-if="iconComponent"
      :is="iconComponent"
      class="input__icon"
      :stroke-width="2"
      aria-hidden="true"
    />

    <input
      v-model="localValue"
      :id="id"
      :name="name"
      :type="inputType"
      :placeholder="placeholder"
      :disabled="disabled"
      :autocomplete="autocomplete"
      class="input__field"
    />

    <button
      v-if="hasPasswordToggle"
      type="button"
      class="input__action"
      :aria-label="showPassword ? 'Hide password' : 'Show password'"
      :aria-pressed="showPassword"
      @click="togglePassword"
    >
      <EyeOff v-if="showPassword" :stroke-width="1.5" />

      <Eye v-else :stroke-width="1.5" />
    </button>
  </div>
</template>

<style scoped>
.input {
  display: flex;
  align-items: center;
  gap: 0.5rem;

  width: 100%;
  height: 2.5rem;
  padding: 0.75rem;
  margin: 0;
  box-sizing: border-box;

  border-radius: 0.75rem;

  background: var(--color-bg);

  box-shadow:
    inset 0 0 0 2px var(--color-border),
    var(--inset-sm);

  transition:
    box-shadow 0.2s ease,
    border-color 0.2s ease;
}

/* Corner smoothing */
@supports (corner-shape: squircle) {
  .input {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}

.input:focus-within {
  box-shadow:
    inset 0 0 0 2px var(--color-accent),
    var(--inset-sm);
}

.input__icon {
  flex: none;
  width: 1.125rem;
  height: 1.125rem;
  color: var(--color-text-muted);
  stroke-width: 1.5;
}

.input__field {
  flex: 1;
  min-width: 0;

  appearance: none;
  border: none;
  outline: none;
  background: transparent;

  font: inherit;
  font-size: 1rem;
  font-weight: 300;
  line-height: 1;

  text-box-trim: trim-both;

  color: var(--color-text-tertiary);
}

.input__field::-ms-reveal {
  display: none;
}

.input__field::placeholder {
  color: var(--color-text-muted);
  opacity: 1;
}

.input__field:disabled {
  cursor: not-allowed;
}

.input:has(.input__field:disabled) {
  opacity: 0.5;
  cursor: not-allowed;
}

.input__field:-webkit-autofill,
.input__field:-webkit-autofill:hover,
.input__field:-webkit-autofill:focus {
  -webkit-text-fill-color: var(--color-text-tertiary);
  transition: background-color 9999s;
}

.input__action {
  appearance: none;
  border: none;
  outline: none;
  padding: 0;
  margin: 0;

  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex: none;

  width: 1.125rem;
  height: 1.125rem;

  background: transparent;
  color: var(--color-text-muted);

  cursor: pointer;

  transition:
    color 0.2s ease,
    opacity 0.2s ease;
}

.input__action:hover {
  color: var(--color-text);
}

.input__action:focus-visible {
  color: var(--color-accent);
}

.input__action svg {
  width: 100%;
  height: 100%;
  stroke-width: 1.5;
}
</style>
