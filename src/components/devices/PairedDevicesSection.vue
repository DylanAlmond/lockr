<script setup lang="ts">
import { Laptop, ShieldOff, Smartphone } from '@lucide/vue';
import Button from '../ui/Button.vue';
import type { PairedDevice } from '../../types';
import { isDesktopPlatform, lastSyncLabel } from '../../util/devices';

defineProps<{
  devices: PairedDevice[];
  loading: boolean;
  syncing: boolean;
}>();

const emit = defineEmits<{
  (e: 'pair'): void;
  (e: 'join'): void;
  (e: 'sync', device: PairedDevice): void;
  (e: 'revoke', device: PairedDevice): void;
}>();
</script>

<template>
  <div class="section-heading">
    <div>
      <h2>Paired Devices</h2>
      <p class="section-hint">
        Sync happens explicitly, one Device at a time. Discovery alone never grants access.
      </p>
    </div>
    <div class="pairing-actions">
      <Button variant="accent" size="small" @click="emit('pair')">Pair a new Device</Button>
      <Button variant="outline" size="small" @click="emit('join')">Join a Device</Button>
    </div>
  </div>

  <p v-if="loading && devices.length === 0" class="muted">Loading Devices…</p>
  <p v-else-if="devices.length === 0" class="empty-state">
    No other Devices are paired yet. Pair one to start syncing your Vaults.
  </p>

  <ul v-else class="device-list">
    <li
      v-for="device in devices"
      :key="device.device_id"
      class="device-card"
      :class="{ 'device-card--revoked': device.revoked }"
    >
      <div class="device-icon">
        <Laptop v-if="isDesktopPlatform(device.platform)" :size="20" aria-hidden="true" />
        <Smartphone v-else :size="20" aria-hidden="true" />
      </div>

      <div class="device-info">
        <div class="device-name-row">
          <strong>{{ device.name }}</strong>
          <span v-if="device.revoked" class="tag tag--danger">Revoked</span>
          <span v-else class="tag tag--paired">Paired</span>
        </div>
        <p class="device-platform">{{ device.platform }}</p>
        <p class="device-last-sync">{{ lastSyncLabel(device) }}</p>
      </div>

      <div class="device-actions">
        <Button
          v-if="!device.revoked"
          variant="accent"
          size="small"
          :disabled="syncing"
          @click="emit('sync', device)"
        >
          Sync now
        </Button>
        <Button
          v-if="!device.revoked"
          variant="label"
          size="small"
          :icon-component="ShieldOff"
          :disabled="syncing"
          @click="emit('revoke', device)"
        >
          Revoke
        </Button>
      </div>
    </li>
  </ul>
</template>

<style scoped src="./device-card.css"></style>

<style scoped>
.pairing-actions {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
}
</style>
