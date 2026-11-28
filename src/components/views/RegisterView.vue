<script setup lang="ts">
import { computed, ref } from 'vue';
import Button from '../../components/ui/Button.vue';
import Input from '../../components/ui/Input.vue';
import { useRouter } from 'vue-router';
import { useUser } from '../../composables/useUser.ts';
import { LockOpen } from '@lucide/vue';

const { register } = useUser();
const router = useRouter();

/** Mirrors the backend floor; the score check remains backend-only. */
const MASTER_PASSWORD_MIN_LENGTH = 12;

const name = ref('');
const masterPassword = ref('');
const error = ref('');
const isLoading = ref(false);

const canSubmit = computed(
  () => !!name.value && masterPassword.value.length >= MASTER_PASSWORD_MIN_LENGTH
);

async function handleRegister() {
  if (!name.value || !masterPassword.value) {
    error.value = 'Please enter a name and a master password.';
    return;
  }

  isLoading.value = true;
  error.value = '';

  try {
    const vaults = await register(name.value, masterPassword.value);
    if (vaults && vaults.length > 0) {
      router.push('/all-items');
    } else {
      error.value = 'Failed to create a new user.';
      isLoading.value = false;
    }
  } catch (e) {
    console.error(e);
    error.value = String(e);
    isLoading.value = false;
  }
}
</script>

<template>
  <form @submit.prevent="handleRegister" class="login-form">
    <Input
      v-model="name"
      type="text"
      placeholder="Enter a new user name..."
      name="name"
      autofocus
    />

    <Input
      v-model="masterPassword"
      type="password"
      placeholder="Enter a new master password..."
      name="master_password"
    />

    <p class="password-hint">
      At least {{ MASTER_PASSWORD_MIN_LENGTH }} characters and not easily guessed. A passphrase of
      several words is strongest.
    </p>

    <Button
      type="submit"
      variant="solid"
      :icon-component="LockOpen"
      :disabled="isLoading || !canSubmit"
      fill
    >
      <span v-if="isLoading">Creating...</span>
      <span v-else>Create & Unlock</span>
    </Button>
  </form>

  <p v-if="error" class="error-text">{{ error }}</p>
</template>

<style scoped>
.password-hint {
  margin: 0;
  font-size: 0.8125rem;
  line-height: 1.4;
  color: var(--color-text-secondary);
  text-align: center;
}
</style>
