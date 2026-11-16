<script setup lang="ts">
import { ShieldCheck } from '@lucide/vue';
import type { PairedDevice } from '../../types';

defineProps<{
  device: PairedDevice;
}>();
</script>

<template>
  <section class="this-device-note">
    <ShieldCheck :size="16" aria-hidden="true" />
    <p>
      This Device is <strong>{{ device.name }}</strong> ({{ device.platform }}). Its identity is a
      fresh id per installation; a reinstall joins as a new Device.
    </p>
  </section>
</template>

<style scoped>
.this-device-note {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
  color: var(--color-text-muted);
  font-size: 0.75rem;
  line-height: 1.5;
}
</style>
