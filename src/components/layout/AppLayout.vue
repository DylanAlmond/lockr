<script setup lang="ts">
import Sidebar from './Sidebar.vue';
</script>

<template>
  <div class="app-layout">
    <Sidebar />

    <main class="content-area">
      <router-view name="list" />
    </main>

    <aside class="panel-area">
      <router-view name="panel" />
    </aside>
  </div>
</template>

<style scoped></style>
