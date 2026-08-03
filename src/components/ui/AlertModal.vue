<script setup lang="ts">
import Button from './Button.vue';

// Define strict types for the props this custom modal accepts
const props = defineProps<{
  title: string;
  message: string;
  actionLabel?: string;
}>();

// Define emits so the parent wrapper can listen for them
const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();
</script>

<template>
  <article class="container">
    <header>
      <h2>{{ title }}</h2>
    </header>

    <main>
      <p>{{ message }}</p>
    </main>

    <footer>
      <Button variant="outline" @click="$emit('close')">Cancel</button>
      <Button @click="$emit('confirm')">{{ props.actionLabel || 'Confirm' }}</button>
    </footer>
  </article>
</template>

<style scoped>
.container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1.5rem;
}

header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 0.5rem;
}

header > h2 {
  font-size: 1.5rem;
  font-family: var(--font-geo);
}

main {
  text-align: center;
  color: var(--color-text-secondary);
  line-height: 1.5rem;
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
}
</style>
