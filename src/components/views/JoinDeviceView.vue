<script setup lang="ts">
import { computed, onMounted, ref } from 'vue';
import { useRouter } from 'vue-router';
import { KeyRound, TriangleAlert } from '@lucide/vue';
import Button from '../ui/Button.vue';
import PairingJoinPanel from '../devices/PairingJoinPanel.vue';
import { useUser } from '../../composables/useUser';
import { joinGate } from '../../util/pairing';
import type { User } from '../../types';

const router = useRouter();
const { getUsers, fetchUser, error: userError } = useUser();

const localUsers = ref<User[]>([]);
const loading = ref(true);
const loadFailed = ref(false);

const gate = computed(() => joinGate(localUsers.value));

async function loadUsers() {
  loading.value = true;
  loadFailed.value = false;

  const listed = await getUsers();
  if (listed) {
    localUsers.value = listed;
  } else {
    loadFailed.value = true;
  }

  loading.value = false;
}

onMounted(loadUsers);

/**
 * `complete_pairing` signs this Device in, so the app must adopt that User
 * even while the panel is still showing the first Vault transfer.
 */
async function handlePaired() {
  await fetchUser();
}

function handleClose(result?: { paired?: boolean }) {
  void router.replace(result?.paired ? '/all-items' : '/auth');
}
</script>

<template>
  <section class="join-view thin-scrollbar">
    <header class="join-header">
      <div class="join-title">
        <KeyRound :size="22" aria-hidden="true" />
        <h1>Join a Device</h1>
      </div>
      <p>
        Joining is the other half of pairing. This Device has no User yet; it takes on the User
        and Vaults of a Device you have already set up.
      </p>
    </header>

    <p v-if="loading" class="join-loading">Checking this Device…</p>

    <div v-else-if="loadFailed" class="join-notice join-notice--error">
      <TriangleAlert :size="20" aria-hidden="true" />
      <div>
        <strong>This Device could not be checked</strong>
        <p>{{ userError ?? 'The local User list is unavailable.' }}</p>
        <Button variant="outline" size="small" @click="loadUsers">Try again</Button>
      </div>
    </div>

    <div v-else-if="!gate.canJoin" class="join-notice">
      <TriangleAlert :size="20" aria-hidden="true" />
      <div>
        <strong>This Device cannot join</strong>
        <p>{{ gate.blockedReason }}</p>
        <p>
          Joining is only for a Device with no User. To add another Device, sign in here and
          choose <strong>Pair a new Device</strong> from the Devices screen.
        </p>
        <Button variant="outline" size="small" @click="router.replace('/auth')">
          Back to sign in
        </Button>
      </div>
    </div>

    <template v-else>
      <div class="join-requirements">
        <h2>Before you start</h2>
        <ul>
          <li>On the other Device, sign in and choose <strong>Pair a new Device</strong>.</li>
          <li>It shows a one-time pairing code and its Device id; both are needed.</li>
          <li>Keep this Device awake. The code expires after about a minute.</li>
          <li>Use the same master password you use on the other Device.</li>
        </ul>
      </div>

      <div class="join-card">
        <PairingJoinPanel @close="handleClose" @paired="handlePaired" />
      </div>
    </template>
  </section>
</template>

<style scoped>
.join-view {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  box-sizing: border-box;
  width: 100%;
  max-width: 34rem;
  max-height: calc(100vh - 12rem);
  padding: 0 1rem;
  overflow-y: auto;
}

.join-header {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  color: var(--color-bg);

  p {
    font-size: 0.875rem;
    line-height: 1.5;
    opacity: 0.9;
  }
}

.join-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;

  h1 {
    font-family: var(--font-geo);
    font-size: 1.5rem;
    font-weight: 500;
  }
}

.join-loading {
  color: var(--color-bg);
  font-size: 0.875rem;
}

.join-notice {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 1rem;
  border-radius: 0.625rem;
  background: rgb(255 255 255 / 0.96);
  color: hsl(30 60% 28%);
  border: 1px solid hsl(38 80% 78%);
  font-size: 0.8125rem;
  line-height: 1.5;

  > div {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 0.5rem;
    flex: 1;
  }

  strong {
    color: hsl(30 60% 24%);
  }
}

.join-notice--error {
  color: hsl(0 60% 30%);
  border-color: hsl(0 70% 80%);
}

.join-requirements {
  padding: 0.875rem 1rem;
  border-radius: 0.625rem;
  background: rgb(255 255 255 / 0.14);
  color: var(--color-bg);
  font-size: 0.8125rem;
  line-height: 1.5;

  h2 {
    font-family: var(--font-geo);
    font-size: 0.9375rem;
    font-weight: 500;
    margin-bottom: 0.375rem;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding-left: 1.125rem;
  }
}

.join-card {
  padding: 1.25rem;
  border-radius: 0.625rem;
  background: var(--color-bg);
  box-shadow: 0 0.5rem 2rem rgb(0 0 0 / 0.12);
}
</style>
