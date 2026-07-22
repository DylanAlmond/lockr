<script setup lang="ts">
import { onMounted, ref } from 'vue';
import Button from '../../components/ui/Button.vue';
import Input from '../../components/ui/Input.vue';
import { useRouter } from 'vue-router';
import { useUser } from '../../composables/useUser.ts';
import Logo from '../../assets/logo.svg';
import Titlebar from '../layout/Titlebar.vue';
import { LockOpen } from '@lucide/vue';

const { login, register, fetchUser } = useUser();
const router = useRouter();

const name = ref('');
const masterPassword = ref('');
const error = ref('');
const isLoading = ref(false);
const hasUser = ref(false);

onMounted(async () => {
  hasUser.value = !!(await fetchUser());
});

async function handleLogin() {
  if (!masterPassword.value) {
    error.value = 'Please enter a master password.';
    return;
  }

  isLoading.value = true;
  error.value = '';

  try {
    const vaults = await login(masterPassword.value);

    if (vaults && vaults.length > 0) {
      router.push('/all-items');
    } else {
      error.value = 'Failed to unlock vaults. Is the password correct?';
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    // Add a small delay so the user sees the success before the screen switches
    setTimeout(() => {
      isLoading.value = false;
    }, 300);
  }
}

async function handleRegister() {
  if (!name.value) {
    error.value = 'Please enter a name.';
    return;
  }

  if (!masterPassword.value) {
    error.value = 'Please enter a master password.';
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
    }
  } catch (e) {
    error.value = String(e);
  } finally {
    // Add a small delay so the user sees the success before the screen switches
    setTimeout(() => {
      isLoading.value = false;
    }, 300);
  }
}
</script>

<template>
  <div class="wrapper">
    <Titlebar />

    <div class="login-container">
      <Logo class="logo" />

      <form v-if="hasUser" @submit.prevent="handleLogin" class="login-form">
        <Input
          v-model="masterPassword"
          type="password"
          placeholder="Enter your master password..."
          name="master_password"
        />

        <Button
          type="submit"
          variant="accent"
          :icon-component="LockOpen"
          :disabled="isLoading || !masterPassword"
          fill
        >
          <span v-if="isLoading">Loading...</span>
          <span v-else>Unlock</span>
        </Button>
      </form>

      <form v-else @submit.prevent="handleRegister" class="login-form">
        <Input v-model="name" type="text" placeholder="Enter a new user name..." name="name" />

        <Input
          v-model="masterPassword"
          type="password"
          placeholder="Enter a new master password..."
          name="master_password"
        />

        <Button
          type="submit"
          variant="accent"
          :icon-component="LockOpen"
          :disabled="isLoading || !masterPassword"
          fill
        >
          <span v-if="isLoading">Loading...</span>
          <span v-else>Unlock</span>
        </Button>
      </form>

      <p v-if="error" class="error-text">{{ error }}</p>
    </div>
  </div>
</template>

<style scoped>
.wrapper {
  background: var(--color-bg);
  height: 100%;
  width: 100%;
}

.login-container {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  gap: 1rem;
  height: 100%;
  width: 100%;
  padding-bottom: 5rem;
}

.title-bar {
  border: none;
}

.logo {
  height: 7.5rem;
  width: auto;
}

.login-form {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-width: 25rem;
  gap: 1rem;
}
</style>
