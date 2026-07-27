<script setup lang="ts">
import { ref, onUnmounted } from 'vue';
import { Copy, Check } from '@lucide/vue';
import Button from './Button.vue';

type CopyableValue = string | null | (() => string | null | Promise<string | null>);

const props = defineProps<{
  label: string;
  displayValue: string | null;
  copyValue?: CopyableValue;
}>();

const copied = ref(false);
let cooldownTimer: ReturnType<typeof setTimeout> | null = null;

async function handleCopy() {
  let valueToCopy: string | null = null;

  // Check if copyValue is a function (e.g., fetching a secret)
  if (typeof props.copyValue === 'function') {
    valueToCopy = await props.copyValue();
  } else {
    // Fall back to displayValue if copyValue isn't explicitly provided
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
      <span v-if="copied" class="copied">Copied!</span>
      <span v-else>{{ displayValue || 'No Value' }}</span>
    </div>

    <div class="field-actions">
      <!-- Slot for extra buttons like Eye/EyeOff or Password Strength -->
      <slot name="actions" />

      <Button
        :aria-label="`Copy ${label}`"
        icon-only
        variant="outline"
        size="small"
        :disabled="!displayValue"
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
</style>
