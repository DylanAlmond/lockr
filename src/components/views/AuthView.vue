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

  & * {
    fill: white;
  }
}

.wrapper {
  height: 100%;
  width: 100%;
  background: linear-gradient(135deg, #875cff 0%, #7542ff 100%), #ffffff;
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

  & * {
    fill: white !important;
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
</style>
