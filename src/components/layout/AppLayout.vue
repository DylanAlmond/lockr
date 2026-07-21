<script setup lang="ts">
import { ArrowLeft, ArrowRight, Plus, Search } from '@lucide/vue';
import Input from '../ui/Input.vue';
import Sidebar from './Sidebar.vue';
import Button from '../ui/Button.vue';
import { useRouterHistory } from '../../composables/useRouterHistory.ts';
import { useRouter } from 'vue-router';
import Titlebar from './Titlebar.vue';

const router = useRouter();
const { canGoBack, canGoForward, goBack, goForward } = useRouterHistory(router);
</script>

<template>
  <div class="app-layout">
    <Sidebar />

    <div class="main-wrapper">
      <Titlebar>
        <div class="nav-button-container">
          <Button
            name="navigate-back"
            variant="label"
            :icon-component="ArrowLeft"
            icon-only
            :disabled="!canGoBack"
            @click="goBack"
          />
          <Button
            name="navigate-forward"
            variant="label"
            :icon-component="ArrowRight"
            icon-only
            :disabled="!canGoForward"
            @click="goForward"
          />
        </div>

        <Input
          class="nav-search"
          name="nav-search"
          type="search"
          :icon-component="Search"
          placeholder="Search..."
        />
        <Button name="Create" variant="accent" :icon-component="Plus">Create</Button>
      </Titlebar>

      <div class="content">
        <main>
          <router-view name="list" />
        </main>

        <aside>
          <router-view name="panel" />
        </aside>
      </div>
    </div>
  </div>
</template>

<style scoped>
.app-layout {
  display: flex;
  width: 100%;
  height: 100%;
}

.main-wrapper {
  display: flex;
  flex-direction: column;
  flex: 1;
}

.content {
  display: grid;
  grid-template-columns: 440px 1fr;
}

.nav-button-container {
  flex-shrink: 0;
}

.nav-search {
  flex-shrink: 1;
}
</style>
