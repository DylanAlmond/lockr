<script setup lang="ts">
import Sidebar from './Sidebar.vue';
import Titlebar from './Titlebar.vue';
</script>

<template>
  <div class="app-shell">
    <Sidebar />

    <div class="main-wrapper">
      <Titlebar>
        <slot name="titlebar" />
      </Titlebar>

      <div class="content">
        <slot />
      </div>
    </div>
  </div>
</template>

<style scoped>
.app-shell {
  display: flex;
  width: 100%;
  height: 100%;
  overflow: hidden;
}

.main-wrapper {
  display: flex;
  flex-direction: column;
  flex: 1;
  background: var(--color-bg);
}

.content {
  display: flex;
  flex: 1;
  min-height: 0;
}
</style>
