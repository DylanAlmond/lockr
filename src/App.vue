<script setup lang="ts">
import { RouterView } from 'vue-router';
import BaseModal from './components/ui/BaseModal.vue';
import { useSessionTimeout } from './composables/useSessionTimeout';

useSessionTimeout();
</script>

<template>
  <RouterView />
  <BaseModal />
</template>
