<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import Button from '../../components/ui/Button.vue';
import Input from '../../components/ui/Input.vue';
import { useRoute, useRouter } from 'vue-router';
import { useUser } from '../../composables/useUser.ts';
import { LockOpen, Plus } from '@lucide/vue';
import { User } from '../../types/index.ts';

const { login, getUsers } = useUser();
const route = useRoute();
const router = useRouter();

const masterPassword = ref('');
const error = ref('');
const isLoading = ref(false);

const selectedUser = ref<User | null>(null);
const availableUsers = ref<User[]>([]);
const hasUser = computed(() => !!selectedUser.value);

onMounted(async () => {
  availableUsers.value = (await getUsers()) || [];
});

function selectUser(user: User) {
  selectedUser.value = user;

  router.push({
    path: '/auth',
    query: { user: user.id }
  });
}

async function handleLogin() {
  if (!availableUsers.value.length) {
    error.value = 'No users exist.';
    return;
  }

  if (!selectedUser.value) {
    error.value = 'No user selected.';
    return;
  }

  if (!masterPassword.value) {
    error.value = 'Please enter a master password.';
    return;
  }

  isLoading.value = true;
  error.value = '';

  try {
    const vaults = await login(selectedUser.value.id, masterPassword.value);

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

watch(
  () => route.query.user,
  (userId) => {
    if (!userId) {
      selectedUser.value = null;
      return;
    }

    selectedUser.value = availableUsers.value.find((u) => u.id === userId) || null;
  },
  { immediate: true }
);
</script>

<template>
  <form v-if="hasUser" @submit.prevent="handleLogin" class="login-form">
    <div class="user-profile">
      <div class="user-button">
        <img
          v-if="selectedUser?.icon"
          :src="selectedUser.icon"
          alt="User icon"
          class="icon-image"
        />
        <span v-else class="icon-text"> {{ (selectedUser?.name || 'No User ')[0] }} </span>
      </div>

      <span>{{ selectedUser?.name }}</span>
    </div>

    <Input
      class="password-input"
      v-model="masterPassword"
      type="password"
      placeholder="Enter your master password..."
      name="master_password"
    />

    <Button
      type="submit"
      variant="solid"
      :icon-component="LockOpen"
      :disabled="isLoading || !masterPassword"
      fill
    >
      <span v-if="isLoading">Loading...</span>
      <span v-else>Unlock</span>
    </Button>
  </form>

  <div v-else>
    <ul class="user-list">
      <li v-for="user in availableUsers" :key="user.id">
        <div class="user-profile">
          <button class="user-button" @click="selectUser(user)" :aria-label="user.name">
            <img v-if="user?.icon" :src="user.icon" alt="User icon" class="icon-image" />
            <span v-else class="icon-text"> {{ (user?.name || 'No User ')[0] }} </span>
          </button>

          <span>{{ user.name }}</span>
        </div>
      </li>

      <div class="user-profile">
        <button
          @click="router.push('/auth/new')"
          class="user-button new-user-button"
          aria-label="New User"
        >
          <Plus :size="40" />
        </button>

        <span>New User</span>
      </div>
    </ul>
  </div>

  <p v-if="error" class="error-text">{{ error }}</p>
</template>

<style scoped>
.user-list {
  display: flex;
  list-style: none;
  align-items: center;
  gap: 1.5rem;
}

.user-button {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 6rem;
  height: 6rem;
  aspect-ratio: 1/1;

  font-size: 2.5rem;
  font-family: var(--font-geo);
  font-weight: 500;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);

  cursor: pointer;
  border: none;
  transition: all 0.2s ease;
  overflow: hidden;

  border-radius: 0.375rem;

  .icon-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 0.375rem;
  }

  .icon-text {
    pointer-events: none;
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }

  &:hover:not(:disabled) {
    opacity: 0.8;
  }

  &:active:not(:disabled) {
    opacity: 0.5;
  }

  &:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  &:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
}

.new-user-button {
  background-color: transparent;
  color: var(--color-bg);

  &:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--color-bg);
  }
}

.login-form {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-width: 22.5rem;
  gap: 1rem;
}

.login-form :deep(.input) {
  background-color: var(--color-bg);
}

.login-form :deep(button) {
  color: var(--color-accent);
}

.login-form :deep(.input),
.login-form :deep(button) {
  transition: all 0.2s ease;
  &:focus-within {
    box-shadow: inset 0 0 0 2px var(--color-accent-muted);
  }
}

.user-profile {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.75rem;
  margin: auto;
  margin-bottom: 1rem;
  font-weight: 300;
  font-family: var(--font-geo);

  color: var(--color-bg);
}
</style>
