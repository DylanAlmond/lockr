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

const availableUsers = ref<User[]>([]);
const selectedUser = ref<User | null>(null);

const hasUser = computed(() => {
  error.value = '';
  return !!selectedUser.value;
});

const scrollContainer = ref<HTMLElement | null>(null);

onMounted(async () => {
  availableUsers.value = (await getUsers()) || [];
});

// Translate vertical wheel scroll into horizontal scroll
function handleWheel(e: WheelEvent) {
  if (!scrollContainer.value) return;

  // If it's a vertical scroll (deltaY is non-zero), prevent default and scroll horizontally
  if (e.deltaY !== 0) {
    e.preventDefault();
    scrollContainer.value.scrollLeft += e.deltaY;
  }
}

function selectUser(user: User) {
  selectedUser.value = user;
  router.push({ path: '/auth', query: { user: user.id } });
}

async function handleLogin() {
  if (!selectedUser.value || !masterPassword.value) {
    error.value = 'Please select a user and enter a master password.';
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
      isLoading.value = false;
    }
  } catch (e) {
    error.value = String(e);
    isLoading.value = false;
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
  <!-- Login Form (User Selected) -->
  <form v-if="hasUser" @submit.prevent="handleLogin" class="login-form">
    <div class="user-profile">
      <div class="user-button">
        <img
          v-if="selectedUser?.icon"
          :src="selectedUser.icon"
          alt="User icon"
          class="icon-image"
        />
        <span v-else class="icon-text">{{ (selectedUser?.name || 'U')[0] }}</span>
      </div>
      <span>{{ selectedUser?.name }}</span>
    </div>

    <Input
      v-model="masterPassword"
      type="password"
      placeholder="Enter your master password..."
      name="master_password"
      autofocus
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

  <!-- User Selection List -->
  <div
    v-else
    ref="scrollContainer"
    class="user-list-wrapper no-scrollbar"
    @wheel.passive="handleWheel"
  >
    <ul class="user-list no-scrollbar">
      <li v-for="user in availableUsers" :key="user.id">
        <div class="user-profile">
          <button class="user-button" @click="selectUser(user)" :aria-label="user.name">
            <img v-if="user?.icon" :src="user.icon" alt="User icon" class="icon-image" />
            <span v-else class="icon-text">{{ (user?.name || 'U')[0] }}</span>
          </button>
          <span>{{ user.name }}</span>
        </div>
      </li>

      <li>
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
      </li>
    </ul>
  </div>

  <p v-if="error" class="error-text">{{ error }}</p>
</template>

<style scoped>
.user-list-wrapper {
  display: flex;
  align-items: center;
  width: 100%;
  max-width: 40rem;
  overflow-x: auto;
  overflow-y: hidden;
  padding: 1.5rem 4rem;
  scroll-snap-type: x proximity;
  scroll-behavior: smooth;
  mask-image: linear-gradient(to right, transparent 0%, black 8%, black 92%, transparent 100%);

  -webkit-mask-image: linear-gradient(
    to right,
    transparent 0%,
    black 8%,
    black 92%,
    transparent 100%
  );
}

.user-list {
  display: flex;
  list-style: none;
  align-items: center;
  gap: 1.5rem;
  padding: 0 1rem;
  width: max-content;
  margin: 0 auto;
}

.user-profile {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.75rem;
  font-weight: 300;
  font-family: var(--font-geo);
  color: var(--color-bg);
  scroll-snap-align: center;
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
}

.user-button .icon-image {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.user-button:hover:not(:disabled) {
  opacity: 0.8;
}

.user-button:active:not(:disabled) {
  opacity: 0.5;
}

.user-button:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--color-accent);
}

.new-user-button {
  background-color: transparent;
  color: var(--color-bg);
  border: 2px dashed var(--color-bg);
}

.new-user-button:focus-visible {
  outline: none;
  box-shadow: inset 0 0 0 2px var(--color-bg);
}

@supports (corner-shape: squircle) {
  .user-button {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}
</style>
