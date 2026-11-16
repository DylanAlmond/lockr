<script setup lang="ts">
import { TriangleAlert } from '@lucide/vue';
import Button from '../ui/Button.vue';

defineProps<{
  error: string;
}>();

const emit = defineEmits<{
  (e: 'recover'): void;
}>();
</script>

<template>
  <section class="identity-banner">
    <TriangleAlert :size="22" aria-hidden="true" />
    <div>
      <strong>This Device’s identity is damaged</strong>
      <p>{{ error }}</p>
    </div>
    <Button variant="danger" size="small" @click="emit('recover')">
      Start with a new identity
    </Button>
  </section>
</template>

<style scoped>
.identity-banner {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.875rem 1rem;
  border-radius: 0.625rem;
  font-size: 0.8125rem;
  line-height: 1.5;
  background: hsl(0 70% 97%);
  border: 1px solid hsl(0 70% 85%);
  color: hsl(0 60% 30%);

  > div {
    flex: 1;
  }

  p {
    margin-top: 0.25rem;
    overflow-wrap: anywhere;
  }
}
</style>
