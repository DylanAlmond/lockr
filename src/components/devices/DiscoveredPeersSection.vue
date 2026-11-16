<script setup lang="ts">
import { computed } from 'vue';
import { RefreshCw, Wifi, WifiOff } from '@lucide/vue';
import Button from '../ui/Button.vue';
import type { DiscoveredPeer, PairedDevice } from '../../types';
import {
  deviceForPeer,
  peerLabel,
  peerStatus,
  thisDeviceOf,
  visiblePeersOf
} from '../../util/devices';

const props = defineProps<{
  peers: DiscoveredPeer[];
  devices: PairedDevice[];
  error: string | null;
  loading: boolean;
  syncing: boolean;
}>();

const emit = defineEmits<{
  (e: 'refresh'): void;
  (e: 'sync', device: PairedDevice): void;
  (e: 'pair'): void;
}>();

const visiblePeers = computed(() => visiblePeersOf(props.peers, thisDeviceOf(props.devices)));

function label(peer: DiscoveredPeer): string {
  return peerLabel(props.devices, peer);
}

function status(peer: DiscoveredPeer) {
  return peerStatus(props.devices, peer);
}

function act(peer: DiscoveredPeer) {
  const device = deviceForPeer(props.devices, peer);
  if (device && status(peer) === 'paired') {
    emit('sync', device);
    return;
  }

  emit('pair');
}
</script>

<template>
  <section class="discovery-section">
    <header class="section-heading">
      <div>
        <h2 class="discovery-title">
          <Wifi v-if="!error" :size="18" aria-hidden="true" />
          <WifiOff v-else :size="18" aria-hidden="true" />
          On this network
        </h2>
        <p class="section-hint">
          Discovery stops when you leave this screen. A discovered Device is not syncable until it
          is paired.
        </p>
      </div>
      <Button
        variant="outline"
        size="small"
        :icon-component="RefreshCw"
        :disabled="loading || syncing"
        @click="emit('refresh')"
      >
        Refresh
      </Button>
    </header>

    <div v-if="error" class="discovery-error">
      <WifiOff :size="18" aria-hidden="true" />
      <div>
        <strong>Discovery is unavailable</strong>
        <p>{{ error }}</p>
      </div>
    </div>

    <p v-else-if="visiblePeers.length === 0" class="empty-state">
      No other Devices are visible right now. Make sure the other Device has Lockr open on the
      Devices screen.
    </p>

    <ul v-else class="peer-list">
      <li v-for="peer in visiblePeers" :key="peer.device_id" class="peer-card">
        <div class="device-icon device-icon--peer">
          <Wifi :size="18" aria-hidden="true" />
        </div>

        <div class="device-info">
          <div class="device-name-row">
            <strong>{{ label(peer) }}</strong>
            <span v-if="status(peer) === 'paired'" class="tag tag--paired">Paired</span>
            <span v-else-if="status(peer) === 'revoked'" class="tag tag--danger">Revoked</span>
            <span v-else class="tag tag--unknown">Unknown</span>
          </div>
          <p class="device-platform">{{ peer.address }}</p>
          <p v-if="status(peer) === 'unknown'" class="device-last-sync">
            Not paired. Seeing it grants no access.
          </p>
          <p v-else-if="status(peer) === 'revoked'" class="device-last-sync">
            Revoked Devices cannot be synced with.
          </p>
        </div>

        <div class="device-actions">
          <Button
            v-if="status(peer) === 'paired'"
            variant="accent"
            size="small"
            :disabled="syncing"
            @click="act(peer)"
          >
            Sync now
          </Button>
          <Button
            v-else-if="status(peer) === 'unknown'"
            variant="outline"
            size="small"
            :disabled="syncing"
            @click="act(peer)"
          >
            Pair…
          </Button>
          <span v-else class="tag tag--danger">No sync</span>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped src="./device-card.css"></style>

<style scoped>
.discovery-section {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.discovery-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
}

.discovery-error {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.875rem 1rem;
  border-radius: 0.625rem;
  background: hsl(38 90% 96%);
  border: 1px solid hsl(38 80% 82%);
  color: hsl(30 60% 28%);

  p {
    margin-top: 0.125rem;
    font-size: 0.8125rem;
    overflow-wrap: anywhere;
  }
}
</style>
