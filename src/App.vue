<script setup lang="ts">
import CreateVault from './components/CreateVault.vue';
import UnlockVault from './components/UnlockVault.vue';
import VaultView from './components/VaultView.vue';
import { useVault } from './composables/useVault.ts';

const { currentVault } = useVault();
</script>

<template>
  <VaultView />

  <hr style="margin: 2rem 0" />

  <div style="display: flex; gap: 2rem">
    <CreateVault />
    <UnlockVault />
  </div>

  <!-- View result -->
  <div v-if="currentVault" style="margin: 2rem; padding: 1rem; background: #eee">
    <h3>Currently Unlocked Vault State:</h3>
    <pre>{{ JSON.stringify(currentVault, null, 2) }}</pre>
  </div>
</template>
