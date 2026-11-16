<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue';
import { onBeforeRouteLeave, useRouter } from 'vue-router';
import { ArrowLeft, RefreshCw } from '@lucide/vue';
import AppShell from '../layout/AppShell.vue';
import Button from '../ui/Button.vue';
import SyncPanel from '../devices/SyncPanel.vue';
import ConflictPrompt from '../devices/ConflictPrompt.vue';
import DuplicateReview from '../devices/DuplicateReview.vue';
import SyncSetEditor from '../devices/SyncSetEditor.vue';
import DeviceIdentityBanner from '../devices/DeviceIdentityBanner.vue';
import PairedDevicesSection from '../devices/PairedDevicesSection.vue';
import DiscoveredPeersSection from '../devices/DiscoveredPeersSection.vue';
import ThisDeviceNote from '../devices/ThisDeviceNote.vue';
import { useDevices } from '../../composables/useDevices';
import { useDiscovery } from '../../composables/useDiscovery';
import { useSyncControl } from '../../composables/useSyncControl';
import { useDeviceAdmin } from '../../composables/useDeviceAdmin';
import { useSync } from '../../composables/useSync';

const router = useRouter();
const { getDeviceIdentityError } = useSync();

const devicesState = useDevices();
const discovery = useDiscovery();
const identityError = ref<string | null>(null);
const duplicateReview = ref<{ refresh: () => void } | null>(null);

const admin = useDeviceAdmin({
  refreshDevices: devicesState.refreshDevices,
  onChanged: devicesState.refreshAll
});

const syncControl = useSyncControl({
  devices: devicesState.devices,
  deviceNames: devicesState.deviceNames,
  refreshDevices: devicesState.refreshDevices,
  onSynced: async () => {
    await devicesState.refreshAfterSync();
    duplicateReview.value?.refresh();
  }
});

const {
  devices,
  loadingDevices,
  pairedPeers,
  thisDevice,
  deviceNames,
  vaultNames,
  syncSetVaults,
  unregistered,
  latestLastSync,
  refreshAll
} = devicesState;

const { peers, discoveryError, loadingPeers, refreshPeers } = discovery;

const {
  navigationBlocked,
  activePeerLabel,
  isSyncing,
  progress,
  syncFailure,
  conflicts,
  moveResidueNotes,
  conflictError,
  isResolvingConflict,
  clearSyncFailure,
  syncNow,
  retryLastSync,
  cancelRunningSync,
  handleResolveConflict,
  resyncFromConflict,
  abandonOpenConflicts
} = syncControl;

const { actionError, openPairing, confirmRevoke, recoverIdentity } = admin;

let unlistenProgress: (() => void) | null = null;

onMounted(async () => {
  unlistenProgress = await syncControl.subscribeProgress();

  identityError.value = await getDeviceIdentityError();
  await devicesState.refreshAll();
  await discovery.beginDiscovery();

  discovery.startPolling();
});

onUnmounted(() => {
  unlistenProgress?.();
  void discovery.finishDiscovery();
});

onBeforeRouteLeave(() => syncControl.blockNavigation() === null);
</script>

<template>
  <AppShell>
    <template #titlebar>
      <Button
        class="back-button"
        aria-label="Back"
        variant="label"
        icon-only
        :icon-component="ArrowLeft"
        @click="router.back()"
      />
      <h1 class="titlebar-heading">Devices</h1>
      <Button
        aria-label="Refresh Devices"
        variant="label"
        icon-only
        :icon-component="RefreshCw"
        :disabled="loadingDevices || isSyncing"
        @click="refreshAll"
      />
    </template>

    <div class="devices-page thin-scrollbar">
      <div class="devices-container">
        <DeviceIdentityBanner
          v-if="identityError"
          :error="identityError"
          @recover="recoverIdentity"
        />

        <p v-if="navigationBlocked" class="navigation-blocked">{{ navigationBlocked }}</p>
        <p v-if="actionError" class="action-error">{{ actionError }}</p>

        <SyncPanel
          :syncing="isSyncing"
          :peer-label="activePeerLabel"
          :progress="progress"
          :failure="syncFailure"
          :last-synced-at="latestLastSync"
          :vault-names="vaultNames"
          @cancel="cancelRunningSync"
          @retry="retryLastSync"
          @dismiss="clearSyncFailure"
          @recover-identity="recoverIdentity"
        />

        <PairedDevicesSection
          :devices="pairedPeers"
          :loading="loadingDevices"
          :syncing="isSyncing"
          @pair="openPairing"
          @sync="syncNow"
          @revoke="confirmRevoke"
        />

        <DiscoveredPeersSection
          :peers="peers"
          :devices="devices"
          :error="discoveryError"
          :loading="loadingPeers"
          :syncing="isSyncing"
          @refresh="refreshPeers"
          @sync="syncNow"
          @pair="openPairing"
        />

        <DuplicateReview
          ref="duplicateReview"
          :vaults="syncSetVaults"
          :notes="moveResidueNotes"
        />

        <SyncSetEditor
          :vaults="syncSetVaults"
          :unregistered="unregistered"
          @changed="refreshAll"
        />

        <ThisDeviceNote v-if="thisDevice" :device="thisDevice" />
      </div>
    </div>

    <ConflictPrompt
      v-if="conflicts"
      :session="conflicts"
      :vault-names="vaultNames"
      :device-names="deviceNames"
      :busy="isResolvingConflict"
      :error="conflictError"
      @resolve="handleResolveConflict"
      @resync="resyncFromConflict"
      @abandon="abandonOpenConflicts"
    />
  </AppShell>
</template>

<style scoped>
.titlebar-heading {
  font-family: var(--font-geo);
  font-size: 1.25rem;
  font-weight: 500;
  margin: auto 0;
  margin-left: auto;
}

.back-button {
  margin-right: auto;
}

.devices-page {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.devices-container {
  display: flex;
  flex-direction: column;
  gap: 2rem;
  max-width: 56rem;
  margin: 0 auto;
  padding: 2rem 1.5rem 3rem;
}

.navigation-blocked {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.875rem 1rem;
  border-radius: 0.625rem;
  font-size: 0.8125rem;
  line-height: 1.5;
  background: hsl(38 90% 96%);
  border: 1px solid hsl(38 80% 82%);
  color: hsl(30 60% 28%);
}

.action-error {
  padding: 0.625rem 0.875rem;
  border-radius: 0.5rem;
  background: hsl(0 70% 97%);
  border: 1px solid hsl(0 70% 85%);
  color: var(--color-red);
  font-size: 0.8125rem;
  overflow-wrap: anywhere;
}
</style>
