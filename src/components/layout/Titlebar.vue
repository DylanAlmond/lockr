<script setup lang="ts">
import { Minus, Square, SquaresUnite, X } from '@lucide/vue';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { onMounted, onUnmounted, ref } from 'vue';

const appWindow = getCurrentWindow();

const isMaximized = ref(false);

let unlisten: (() => void) | undefined;

async function refreshMaximizedState() {
  isMaximized.value = await appWindow.isMaximized();
}

onMounted(async () => {
  await refreshMaximizedState();
  // Keep icon in sync when maximized/restored via dblclick, OS shortcuts, etc.
  unlisten = await appWindow.onResized(() => {
    refreshMaximizedState();
  });
});

onUnmounted(() => {
  unlisten?.();
});
</script>

<template>
  <nav class="title-bar" data-tauri-drag-region @dblclick="appWindow.toggleMaximize()">
    <slot />

    <div class="window-button-container">
      <button @click="appWindow.minimize()"><Minus :size="16" /></button>

      <button @click="appWindow.toggleMaximize()">
        <SquaresUnite v-if="isMaximized" :size="16" />
        <Square v-else :size="16" />
      </button>

      <button @click="appWindow.close()"><X :size="18" /></button>
    </div>
  </nav>
</template>

<style scoped>
.title-bar {
  display: flex;
  column-span: all;
  box-sizing: border-box;

  width: 100%;
  height: 4rem;

  gap: 1.5rem;
  padding: 0.75rem 0.5rem 0.75rem 1rem;

  border-bottom: 1px solid var(--color-border);
}

.window-button-container {
  display: flex;
  flex-shrink: 0;
  margin-left: 0.25rem;

  button {
    display: flex;
    justify-content: center;
    align-items: center;

    border: none;
    outline: none;
    cursor: pointer;

    background-color: var(--color-bg);

    width: 2.5rem;
    height: 100%;

    color: var(--color-text-muted);

    border-radius: 4px;

    transition: all 0.3s ease;

    &:hover {
      /* opacity: 0.5; */
      background-color: var(--color-hover);
    }

    &:focus-visible {
      background-color: var(--color-hover);
    }

    &:disabled {
      opacity: 0.5;
      cursor: not-allowed;
    }
  }
}
</style>
