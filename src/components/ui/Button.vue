<script setup lang="ts">
import { LucideProps } from '@lucide/vue';
import { useAttrs, type Component } from 'vue';

type Variant = 'accent' | 'outline' | 'label' | 'solid' | 'danger' | 'neon';
type Size = 'default' | 'small' | 'xs';

interface Props {
  variant?: Variant;
  size?: Size;
  iconOnly?: boolean;
  iconComponent?: Component;
  iconProps?: LucideProps;
  type?: 'button' | 'submit' | 'reset';
  disabled?: boolean;
  fill?: boolean;
}

defineOptions({ inheritAttrs: false });

withDefaults(defineProps<Props>(), {
  variant: 'accent',
  size: 'default',
  iconOnly: false,
  type: 'button',
  disabled: false
});

// Native attrs (class, aria-*, name, form, title, etc.) forward to the root <button>
const attrs = useAttrs();
</script>

<template>
  <button
    v-bind="attrs"
    :type="type"
    :disabled="disabled"
    :class="[
      'button',
      `button--${variant}`,
      `button--${size}`,
      {
        'button--icon': iconOnly
      },
      { 'button--fill': fill }
    ]"
  >
    <component
      v-if="iconComponent"
      :is="iconComponent"
      v-bind="iconProps"
      class="button__icon"
      aria-hidden="true"
    />

    <span v-if="!iconOnly" class="button__label">
      <slot />
    </span>
  </button>
</template>

<style scoped>
.button {
  /* Size tokens */
  --button-height: 2.5rem;
  --button-padding-x: 1rem;
  --button-padding-y: 0.5rem;
  --button-icon-size: 1.125rem;

  appearance: none;
  border: none;
  outline: none;

  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  gap: 0.5rem;

  width: fit-content;
  height: var(--button-height);
  padding: var(--button-padding-y) var(--button-padding-x);

  position: relative;
  overflow: hidden;

  cursor: pointer;
  user-select: none;

  font: inherit;
  font-size: 1rem;
  line-height: 1;
  color: inherit;

  border-radius: 0.75rem;

  transition: all 0.2s ease;
}

/* Corner smoothing */
@supports (corner-shape: squircle) {
  .button {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}

/* ---------- Sizes ---------- */

.button--default {
  --button-height: 2.5rem;
  --button-icon-size: 1.125rem;
}

.button--small {
  --button-height: 2.25rem;
  --button-icon-size: 1rem;
}

.button--xs {
  --button-height: 2rem;
  --button-icon-size: 1rem;
  --button-padding-x: 0.75rem;
  --button-padding-y: 0.375rem;

  gap: 0.375rem;
  font-size: 0.875rem;
}

.button--fill {
  width: 100%;
}

/* ---------- Icon Only ---------- */

.button--icon {
  --button-icon-size: 1.25rem;

  /* Increase to 24px for labels */
  /* &.button--label {
    --button-icon-size: 1.5rem;
  } */

  width: var(--button-height);
  padding: 0;
  aspect-ratio: 1;
}

/* ---------- Accent ---------- */

.button--accent {
  background: var(--color-accent);
  color: var(--color-bg);
  box-shadow: var(--shadow-sm);
}

/* ---------- Outline ---------- */

.button--outline {
  background: transparent;
  color: var(--color-text);

  box-shadow: inset 0 0 0 1px var(--color-border);
}

/* ---------- Danger ---------- */

.button--danger {
  background: transparent;
  color: var(--color-red);

  box-shadow: inset 0 0 0 1px var(--color-red);
}

/* ---------- Label ---------- */

.button--label {
  background: transparent;
  color: var(--color-text);
}

/* ---------- Solid ---------- */

.button--solid {
  background: var(--color-bg);
  color: var(--color-text);
}

/* ---------- Icon ---------- */

.button__icon {
  flex: 0 0 auto;

  width: var(--button-icon-size);
  height: var(--button-icon-size);

  stroke-width: 2;
}

/* ---------- Neon ---------- */

.button--neon {
  background: var(--color-accent-hover);
  color: var(--color-accent);
}

/* Variant colours */

.button--accent .button__icon {
  color: var(--color-bg);
}

.button--outline .button__icon {
  color: var(--color-text-muted);
}

.button--danger .button__icon {
  color: var(--color-red);
}

.button--label .button__icon {
  color: var(--color-text-muted);
}

/* ---------- Label ---------- */

.button__label {
  white-space: nowrap;
  text-box-trim: trim-both;
  text-box-edge: cap alphabetic;
}

/* ---------- States ---------- */

.button:hover:not(:disabled) {
  opacity: 0.8;
}

.button:active:not(:disabled) {
  opacity: 0.5;
}

.button:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--color-accent);

  &.button--accent {
    box-shadow: inset 0 0 0 2px var(--color-text);
  }
}

.button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
