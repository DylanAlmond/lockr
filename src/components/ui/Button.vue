<script setup lang="ts">
import { LucideProps } from '@lucide/vue';
import type { Component } from 'vue';

type Variant = 'accent' | 'outline' | 'label';
type Size = 'default' | 'small';

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

withDefaults(defineProps<Props>(), {
  variant: 'accent',
  size: 'default',
  iconOnly: false,
  type: 'button',
  disabled: false
});
</script>

<template>
  <button
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

  transition:
    opacity 0.2s ease,
    background-color 0.2s ease,
    box-shadow 0.2s ease,
    border-color 0.2s ease;
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
  background:
    linear-gradient(180deg, rgba(255, 255, 255, 0.2) 0%, rgba(0, 0, 0, 0.2) 100%),
    var(--color-accent);

  color: var(--color-bg);

  box-shadow: var(--shadow-sm);
}

/* ---------- Outline ---------- */

.button--outline {
  background: transparent;
  color: var(--color-text);

  box-shadow: inset 0 0 0 1px var(--color-border);
}

/* ---------- Label ---------- */

.button--label {
  background: transparent;
  color: var(--color-text);
}

/* ---------- Icon ---------- */

.button__icon {
  flex: 0 0 auto;

  width: var(--button-icon-size);
  height: var(--button-icon-size);

  stroke-width: 2;
}

/* Variant colours */

.button--accent .button__icon {
  color: var(--color-bg);
}

.button--outline .button__icon {
  color: var(--color-text-muted);
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
  outline: 2px solid var(--color-accent);
  outline-offset: 2px;
}

.button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
