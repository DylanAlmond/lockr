<script setup lang="ts">
import { KeyRound } from '@lucide/vue';
import PairingSourcePanel from './PairingSourcePanel.vue';

const emit = defineEmits<{
  (e: 'close'): void;
}>();
</script>

<template>
  <article class="pairing-modal">
    <header class="pairing-header">
      <div class="pairing-title">
        <KeyRound :size="22" aria-hidden="true" />
        <h2>Pair a Device</h2>
      </div>
      <p class="pairing-subtitle">
        This Device starts pairing because it already has your User and Vaults. It shows a
        one-time code; the Device you are adding must have no User yet and joins from the
        sign-in screen.
      </p>
    </header>

    <PairingSourcePanel @close="emit('close')" />
  </article>
</template>

<style scoped src="./PairingModal.css"></style>
