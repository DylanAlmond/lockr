<script setup lang="ts">
import { computed, ref } from 'vue';
import Button from './Button.vue';
import Input from './Input.vue';
import { Vault } from '../../types/index.ts';
import useAppStore from '../../stores/appStore.ts';

const props = defineProps<{
  vault: Vault;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'deleted', vaultId: string): void;
}>();

const { deleteVaultById } = useAppStore();

const confirmationInput = ref('');
const masterPassword = ref('');
const formError = ref('');
const isDeleting = ref(false);

const canConfirm = computed(
  () =>
    !isDeleting.value &&
    confirmationInput.value === props.vault.name &&
    masterPassword.value.length > 0
);

async function handleConfirm() {
  if (!canConfirm.value) return;

  isDeleting.value = true;
  formError.value = '';

  try {
    const failure = await deleteVaultById(props.vault.id, masterPassword.value);

    if (failure) {
      formError.value = failure;
      return;
    }

    emit('deleted', props.vault.id);
    emit('close');
  } finally {
    // Do not keep the master password in a component ref after the attempt.
    masterPassword.value = '';
    isDeleting.value = false;
  }
}
</script>

<template>
  <form @submit.prevent="handleConfirm" :aria-busy="isDeleting">
    <article class="container">
      <header>
        <h2>Delete "{{ props.vault.name }}"</h2>
      </header>

      <main>
        <p>All items in this vault will be deleted. This can't be undone from within Lockr.</p>

        <div class="confirmation-field">
          <label for="delete-vault-name-input">
            Type <strong>{{ props.vault.name }}</strong> to confirm
          </label>
          <Input
            id="delete-vault-name-input"
            v-model="confirmationInput"
            type="text"
            autocomplete="off"
            :placeholder="props.vault.name"
            :disabled="isDeleting"
          />
        </div>

        <div class="confirmation-field">
          <label for="delete-vault-password-input">Enter your master password</label>
          <Input
            id="delete-vault-password-input"
            v-model="masterPassword"
            type="password"
            autocomplete="current-password"
            placeholder="Master password"
            :disabled="isDeleting"
          />
        </div>

        <p v-if="formError" class="error-text">{{ formError }}</p>
      </main>

      <footer>
        <Button variant="outline" type="button" :disabled="isDeleting" @click="emit('close')">
          Cancel
        </Button>
        <Button type="submit" :disabled="!canConfirm">
          {{ isDeleting ? 'Deleting…' : 'Delete' }}
        </Button>
      </footer>
    </article>
  </form>
</template>

<style scoped>
.container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1.5rem;
  width: 100%;
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
  display: flex;
  flex-direction: column;
  gap: 1rem;
  text-align: center;
  color: var(--color-text-secondary);
  line-height: 1.5rem;
  width: 100%;
}

.confirmation-field {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.5rem;
  text-align: left;

  > label {
    font-size: 0.875rem;
    color: var(--color-text-secondary);
  }
}

.error-text {
  margin: 0;
  color: var(--color-danger, #e5484d);
  font-size: 0.875rem;
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
  width: 100%;

  /* Keeps the buttons visible while the modal scrolls, matching the other
     modal footers; the shell has no bottom padding, so this owns the spacing */
  position: sticky;
  bottom: 0;
  padding-bottom: 1.5rem;
  background-color: var(--color-bg);
}
</style>
