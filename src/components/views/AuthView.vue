<script setup lang="ts">
import Logo from '../../assets/logo.svg';
import Shield from '../../assets/logo-shield.svg';
import Titlebar from '../layout/Titlebar.vue';
</script>

<template>
  <div class="wrapper">
    <Titlebar class="titlebar" />

    <div class="login-container">
      <Logo class="logo" />
      <router-view />
    </div>

    <Shield class="shield" />
  </div>
</template>

<style scoped>
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
