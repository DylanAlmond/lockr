<script setup lang="ts">
import { ChevronDown, LucideProps } from '@lucide/vue';
import { computed, ref, useAttrs, watch, type Component } from 'vue';
import Dropdown, { type DropdownItem } from './Dropdown.vue';

export interface SelectOption {
  value: string | number;
  label: string;
  icon?: Component;
  disabled?: boolean;
}

type Variant = 'outline' | 'label';
type Size = 'default' | 'small';

interface Props {
  variant?: Variant;
  size?: Size;
  modelValue?: string | number | null;
  options: SelectOption[];
  iconComponent?: Component;
  iconProps?: LucideProps;
  placeholder?: string;
  disabled?: boolean;
  required?: boolean;
  name?: string;
  id?: string;
  fill?: boolean;
}

defineOptions({ inheritAttrs: false });

const props = withDefaults(defineProps<Props>(), {
  variant: 'outline',
  size: 'default',
  modelValue: null,
  placeholder: 'Select an option',
  disabled: false,
  required: false
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string | number): void;
}>();

// class/style go on our own root; everything else (aria-*, data-*, etc.) forwards to the trigger button
const attrs = useAttrs();

const rootAttrs = computed(() => {
  const { class: klass, style } = attrs;
  return { class: klass, style };
});

const triggerAttrs = computed(() => {
  const { class: _class, style: _style, ...rest } = attrs;
  return rest;
});

const selectedOption = computed(() =>
  props.options.find((option) => option.value === props.modelValue)
);

const triggerLabel = computed(() => selectedOption.value?.label ?? props.placeholder);

const dropdownItems = computed<DropdownItem[]>(() =>
  props.options.map((option) => ({
    id: option.value,
    label: option.label,
    icon: option.icon,
    disabled: option.disabled,
    onSelect: () => emit('update:modelValue', option.value)
  }))
);

// Dropdown's slot types its `trigger-props` loosely (e.g. aria-haspopup as `string`); merge and widen here rather than in Dropdown.vue
function mergeTriggerProps(triggerProps: Record<string, unknown>) {
  return { ...triggerAttrs.value, ...triggerProps } as Record<string, unknown>;
}

// The trigger is a <button>, which can't carry `required` itself — a real (but invisible) native <select>
// mirrors the value so the component participates in native constraint validation like a standard select.
const isInvalid = ref(false);

watch(
  () => props.modelValue,
  (value) => {
    if (value) isInvalid.value = false;
  }
);

function handleNativeInvalid() {
  isInvalid.value = true;
}
</script>

<template>
  <div class="select-root" :class="{ 'select-root--fill': fill }" v-bind="rootAttrs">
    <Dropdown :list="dropdownItems" @select="() => {}">
      <template #trigger="{ isOpen, triggerProps }">
        <button
          v-bind="mergeTriggerProps(triggerProps)"
          type="button"
          :id="id"
          :disabled="disabled"
          :aria-required="required || undefined"
          :aria-invalid="isInvalid || undefined"
          :class="[
            'select-trigger',
            `select-trigger--${variant}`,
            `select-trigger--${size}`,
            { 'select-trigger--placeholder': !selectedOption },
            { 'select-trigger--fill': fill },
            { 'select-trigger--invalid': isInvalid }
          ]"
        >
          <component
            v-if="iconComponent"
            :is="iconComponent"
            v-bind="iconProps"
            class="select-trigger__icon"
            aria-hidden="true"
          />

          <span class="select-trigger__label">{{ triggerLabel }}</span>

          <ChevronDown
            class="select-trigger__chevron"
            :class="{ 'select-trigger__chevron--open': isOpen }"
            aria-hidden="true"
            :size="20"
          />
        </button>
      </template>
    </Dropdown>

    <!-- Sized/positioned to exactly overlay the trigger (rather than clipped to 1px) so that when the browser
         blocks form submission and reports this as invalid, its native validation bubble anchors to the visible
         control instead of an offscreen point. It only ever mirrors modelValue — never wired to a change listener
         or made focusable — so this stays a one-way sync, never a second source of truth. -->
    <select
      v-if="name || required"
      v-bind="attrs"
      class="select-native"
      :name="name"
      :value="modelValue ?? ''"
      :disabled="disabled"
      :required="required"
      tabindex="-1"
      aria-hidden="true"
      @invalid="handleNativeInvalid"
    >
      <option v-if="placeholder" value="" disabled hidden>{{ placeholder }}</option>
      <option v-for="option in options" :key="option.value" :value="option.value">
        {{ option.label }}
      </option>
    </select>
  </div>
</template>

<style scoped>
.select-trigger {
  /* Size tokens */
  --select-height: 2.5rem;
  --select-padding-x: 0.75rem;
  --select-icon-size: 1.125rem;

  appearance: none;
  border: none;
  outline: none;

  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  gap: 0.5rem;

  width: fit-content;
  height: var(--select-height);
  padding: 0 var(--select-padding-x);
  margin: 0;
  box-sizing: border-box;

  cursor: pointer;
  user-select: none;

  font: inherit;
  font-size: 1rem;
  line-height: 1;
  color: var(--color-text);

  border-radius: 0.75rem;

  transition:
    opacity 0.2s ease,
    background-color 0.2s ease,
    box-shadow 0.2s ease,
    border-color 0.2s ease;
}

/* Corner smoothing */
@supports (corner-shape: squircle) {
  .select-trigger {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}

/* ---------- Sizes ---------- */

.select-trigger--default {
  --select-height: 2.5rem;
  --select-icon-size: 1.125rem;
}

.select-trigger--small {
  --select-height: 2.25rem;
  --select-icon-size: 1rem;
}

.select-trigger--fill {
  width: 100%;
}

/* ---------- Outline ---------- */

.select-trigger--outline {
  background: transparent;
  color: var(--color-text);

  box-shadow: inset 0 0 0 1px var(--color-border);
}

.select-trigger--outline:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

/* ---------- Label ---------- */

.select-trigger--label {
  background: transparent;
  box-shadow: none;
}

/* ---------- Icon ---------- */

.select-trigger__icon {
  flex: 0 0 auto;

  width: var(--select-icon-size);
  height: var(--select-icon-size);

  stroke-width: 1.5;
}

/* ---------- Label text ---------- */

.select-trigger__label {
  flex: 1;
  min-width: 0;

  overflow-y: visible;
  text-overflow: ellipsis;
  white-space: nowrap;

  text-align: left;

  text-box-trim: trim-both;
  text-box-edge: cap alphabetic;
}

.select-trigger--placeholder .select-trigger__label {
  color: var(--color-text-muted);
}

/* ---------- Chevron ---------- */

.select-trigger__chevron {
  flex: 0 0 auto;

  color: var(--color-text-muted);
  stroke-width: 1.5;

  transition: transform 0.2s ease;
}

.select-trigger__chevron--open {
  transform: rotate(180deg);
}

/* ---------- States ---------- */

.select-trigger:hover:not(:disabled) {
  opacity: 0.8;
}

.select-trigger:active:not(:disabled) {
  opacity: 0.5;
}

.select-trigger:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.select-trigger:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

/* ---------- Root ---------- */

.select-root {
  position: relative;
  display: inline-block;
}

.select-root--fill {
  display: contents;
  width: 100%;
}

/* ---------- Hidden native select ---------- */

.select-native {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  padding: 0;
  margin: 0;
  border: 0;
  opacity: 0;
  pointer-events: none;
}
</style>
