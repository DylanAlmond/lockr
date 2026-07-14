<script setup lang="ts">
import { RouterLink } from 'vue-router';
import Logo from '../../assets/logo-text.svg';
import { getCurrentWindow } from '@tauri-apps/api/window';

const appWindow = getCurrentWindow();
</script>

<template>
  <header class="sidebar">
    <div class="logo-container" data-tauri-drag-region @dblclick="appWindow.toggleMaximize()">
      <Logo />
    </div>

    <nav class="sidebar-nav">
      <RouterLink :to="{ path: '/all-items' }">All Items</RouterLink>
      <RouterLink :to="{ path: '/favourites' }">Favourites</RouterLink>
      <RouterLink :to="{ path: '/recently-accessed' }">Recently Accessed</RouterLink>
      <RouterLink :to="{ path: '/vaults' }">Vaults</RouterLink>
    </nav>
  </header>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;

  width: 320px;
  height: 100%;

  background: var(--color-bg-nav);
  backdrop-filter: blur(12px);

  border-right: 1px solid var(--color-border);
}

.logo-container {
  display: flex;
  align-items: center;
  box-sizing: border-box;
  width: 100%;
  height: 4rem;

  padding: 0.75rem 1rem;
}

.sidebar-nav {
  display: flex;
  flex-direction: column;
  padding: 1rem;

  gap: 1rem;
}
</style>
