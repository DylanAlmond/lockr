<script setup lang="ts">
import { ref } from 'vue';
import { KeyRound } from '@lucide/vue';
import PairingSourcePanel from './PairingSourcePanel.vue';
import PairingJoinPanel from './PairingJoinPanel.vue';
import { useSync } from '../../composables/useSync';

const props = withDefaults(
  defineProps<{
    initialMode?: 'source' | 'join';
  }>(),
  { initialMode: 'source' }
);

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const { cancelPairing } = useSync();
const mode = ref<'source' | 'join'>(props.initialMode);

/**
 * Switching roles tears down whatever the previous panel opened before the
 * new one mounts. Panels start from a clean, idle state of their own.
 */
async function switchMode(next: 'source' | 'join') {
  if (mode.value === next) return;
  await cancelPairing();
  mode.value = next;
}
</script>

<template>
  <article class="pairing-modal">
    <header class="pairing-header">
      <div class="pairing-title">
        <KeyRound :size="22" aria-hidden="true" />
        <h2>Pair a Device</h2>
      </div>
      <p class="pairing-subtitle">
        Pairing is the only grant of access. Being discoverable on the network never authorises a
        Device.
      </p>
    </header>

    <div class="mode-tabs" role="tablist" aria-label="Pairing role">
      <button
        type="button"
        role="tab"
        :aria-selected="mode === 'source'"
        :class="{ active: mode === 'source' }"
        @click="switchMode('source')"
      >
        This Device already has my Vaults
      </button>
      <button
        type="button"
        role="tab"
        :aria-selected="mode === 'join'"
        :class="{ active: mode === 'join' }"
        @click="switchMode('join')"
      >
        This Device is new
      </button>
    </div>

    <PairingSourcePanel v-if="mode === 'source'" @close="emit('close')" />
    <PairingJoinPanel v-else @close="emit('close')" />
  </article>
</template>

<style scoped src="./PairingModal.css"></style>
