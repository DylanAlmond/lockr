<script setup lang="ts">
import { markRaw, onMounted, ref } from 'vue';
import Logo from '../../assets/logo.svg';
import Shield from '../../assets/logo-shield.svg';
import Titlebar from '../layout/Titlebar.vue';
import { ArrowLeft, TriangleAlert } from '@lucide/vue';
import { useRouter } from 'vue-router';
import Button from '../ui/Button.vue';
import AlertModal from '../ui/AlertModal.vue';
import { useModal } from '../../composables/useModal';
import { useSync } from '../../composables/useSync';

const router = useRouter();
const { openModal } = useModal();
const { getDeviceIdentityError, startWithNewDeviceIdentity } = useSync();

const identityError = ref<string | null>(null);
const authEpoch = ref(0);

onMounted(async () => {
  identityError.value = await getDeviceIdentityError();
});

function recoverIdentity() {
  openModal(markRaw(AlertModal), {
    title: 'Start with a new identity?',
    message:
      'This is one-way. This installation becomes a new Device, other Devices will not recognise it, and the old Device’s entry is not revoked. Revoke the old Device from another Device if it was lost.',
    actionLabel: 'Start with a new identity',
    confirmationValue: 'new identity',
    onClose: () => {},
    onConfirm: async () => {
      const recovered = await startWithNewDeviceIdentity(true);
      if (recovered) {
        identityError.value = null;
        // Re-mount the login form so it picks up the rebuilt User list.
        authEpoch.value += 1;
      }
    }
  });
}
</script>

<template>
  <div class="wrapper">
    <Titlebar class="titlebar">
      <Button
        v-if="router.currentRoute.value.fullPath !== '/auth'"
        class="back-button"
        name="navigate-back"
        aria-label="Back"
        variant="label"
        :icon-component="ArrowLeft"
        icon-only
        @click="router.replace('/auth')"
      />
    </Titlebar>

    <div class="login-container">
      <Logo class="logo" />

      <section v-if="identityError" class="identity-banner">
        <TriangleAlert :size="20" aria-hidden="true" />
        <div>
          <strong>This Device’s identity is damaged</strong>
          <p>{{ identityError }}</p>
        </div>
        <Button variant="solid" size="small" @click="recoverIdentity">
          Start with a new identity
        </Button>
      </section>

      <router-view :key="authEpoch" />
    </div>

    <Shield class="shield" />
  </div>
</template>

<style scoped>
.back-button {
  --button-icon-color: var(--color-bg);
  margin-right: auto;
}

.wrapper {
  height: 100%;
  width: 100%;
  background: linear-gradient(135deg, #875cff 0%, #7542ff 100%), #ffffff;
  position: relative;
  overflow: hidden;
}

.titlebar {
  border: none;
}

.titlebar :deep(.window-button) {
  color: var(--color-accent-hover) !important;
  background-color: transparent !important;
}

.shield {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  width: 240%;
  height: auto;
  opacity: 0.025;
  pointer-events: none;
  z-index: 0;
}

.shield :deep(*) {
  fill: white;
}

.login-container {
  position: relative;
  z-index: 1;
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

.logo {
  height: 7.5rem;
  width: auto;
}

.logo :deep(*) {
  fill: white !important;
}

.identity-banner {
  position: relative;
  z-index: 2;
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  box-sizing: border-box;
  width: 100%;
  max-width: 32rem;
  padding: 0.875rem 1rem;
  border-radius: 0.625rem;
  background: rgb(255 255 255 / 0.94);
  color: hsl(0 60% 30%);
  border: 1px solid hsl(0 70% 80%);
  font-size: 0.8125rem;
  line-height: 1.45;

  > div {
    flex: 1;
  }

  strong {
    color: hsl(0 60% 26%);
  }

  p {
    margin-top: 0.25rem;
    overflow-wrap: anywhere;
  }
}

/* Shared styles for child views (Login & Register) */
.login-container :deep(.login-form) {
  display: flex;
  flex-direction: column;
  width: 100%;
  max-width: 22.5rem;
  gap: 1rem;
}

.login-container :deep(.login-form .input) {
  background-color: var(--color-bg);
}

.login-container :deep(.login-form button) {
  --button-icon-color: var(--color-accent);
  color: var(--color-accent);
}

.login-container :deep(.login-form .input),
.login-container :deep(.login-form button) {
  transition: all 0.2s ease;
}

.login-container :deep(.login-form .input:focus-within),
.login-container :deep(.login-form button:focus-visible) {
  box-shadow: inset 0 0 0 2px var(--color-accent-muted);
}

.login-container :deep(.error-text) {
  color: #ffcccc;
  font-size: 0.875rem;
  text-align: center;
  margin-top: 0.5rem;
  width: 100%;
  max-width: 22.5rem;
}
</style>
