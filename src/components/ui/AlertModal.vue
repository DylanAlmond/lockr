<script setup lang="ts">
import { computed, ref } from 'vue';
import Button from './Button.vue';
import Input from './Input.vue';

const props = defineProps<{
  title: string;
  message: string;
  actionLabel?: string;
  /** If set, the confirm button stays disabled until the user types this value exactly */
  confirmationValue?: string;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

const confirmationInput = ref('');

const isConfirmDisabled = computed(
  () => !!props.confirmationValue && confirmationInput.value !== props.confirmationValue
);
</script>

<template>
  <article class="container">
    <header>
      <h2>{{ title }}</h2>
    </header>

    <main>
      <p>{{ message }}</p>

      <div v-if="confirmationValue" class="confirmation-field">
        <label for="alert-confirmation-input">
          Type <strong>{{ confirmationValue }}</strong> to confirm
        </label>
        <Input
          id="alert-confirmation-input"
          v-model="confirmationInput"
          type="text"
          autocomplete="off"
          :placeholder="confirmationValue"
        />
      </div>
    </main>

    <footer>
      <Button variant="outline" @click="emit('close')">Cancel</Button>
      <Button :disabled="isConfirmDisabled" @click="emit('confirm')">
        {{ props.actionLabel || 'Confirm' }}
      </Button>
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

.confirmation-field {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.5rem;
  margin-top: 1rem;
  text-align: left;

  > label {
    font-size: 0.875rem;
    color: var(--color-text-secondary);
  }
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
}
</style>
