<script setup lang="ts">
import { RouterLink } from 'vue-router';
</script>

<template>
  <nav class="sidebar">
    <RouterLink :to="{ path: '/all-items' }">All Items</RouterLink>
    <RouterLink :to="{ path: '/favourites' }">Favourites</RouterLink>
    <RouterLink :to="{ path: '/recently-accessed' }">Recently Accessed</RouterLink>
    <RouterLink :to="{ path: '/vaults' }">Vaults</RouterLink>
  </nav>
</template>
