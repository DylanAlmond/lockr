<script setup lang="ts">
import { computed, markRaw, onMounted, onUnmounted, ref, watch } from 'vue';
import { onBeforeRouteLeave, useRouter } from 'vue-router';
import {
  ArrowLeft,
  Laptop,
  RefreshCw,
  ShieldCheck,
  ShieldOff,
  Smartphone,
  TriangleAlert,
  Wifi,
  WifiOff
} from '@lucide/vue';
import AppShell from '../layout/AppShell.vue';
import Button from '../ui/Button.vue';
import AlertModal from '../ui/AlertModal.vue';
import PairingModal from '../devices/PairingModal.vue';
import SyncPanel from '../devices/SyncPanel.vue';
import ConflictPrompt from '../devices/ConflictPrompt.vue';
import DuplicateReview from '../devices/DuplicateReview.vue';
import SyncSetEditor from '../devices/SyncSetEditor.vue';
import { useModal } from '../../composables/useModal';
import { useSync } from '../../composables/useSync';
import type {
  ConflictChoice,
  DiscoveredPeer,
  PairedDevice,
  SyncConflict,
  SyncSetView,
  UnregisteredVault
} from '../../types';
import { formatTimestamp } from '../../util/timestamp';
import { shortId } from '../../util/labels';

const router = useRouter();
const { openModal } = useModal();

const {
  error,
  isSyncing,
  syncingPeerId,
  progress,
  syncFailure,
  conflicts,
  duplicateNotes,
  conflictError,
  isResolvingConflict,
  clearSyncFailure,
  listPairedDevices,
  startDiscovery,
  stopDiscovery,
  listDiscoveredPeers,
  revokeDevice,
  startServing,
  stopServing,
  syncWithDevice,
  cancelSync,
  onSyncProgress,
  resolveSyncConflict,
  abandonConflicts,
  getSyncSet,
  listUnregisteredVaults,
  getDeviceIdentityError,
  startWithNewDeviceIdentity
} = useSync();

const devices = ref<PairedDevice[]>([]);
const peers = ref<DiscoveredPeer[]>([]);
const syncSet = ref<SyncSetView | null>(null);
const unregistered = ref<UnregisteredVault[]>([]);
const identityError = ref<string | null>(null);
const discoveryError = ref<string | null>(null);
const discoveryOn = ref(false);
const loadingDevices = ref(false);
const loadingPeers = ref(false);
const navigationBlocked = ref<string | null>(null);
const actionError = ref<string | null>(null);
const lastAttemptPeerId = ref<string | null>(null);
const duplicateReview = ref<{ refresh: () => void } | null>(null);

let pollTimer: ReturnType<typeof setInterval> | null = null;
let unlistenProgress: (() => void) | null = null;

const vaultNames = computed<Record<string, string>>(() => {
  const names: Record<string, string> = {};
  for (const vault of syncSet.value?.vaults ?? []) {
    names[vault.vault_id] = vault.name?.trim() || `Vault ${vault.vault_id.slice(0, 8)}`;
  }
  return names;
});

const deviceNames = computed<Record<string, string>>(() => {
  const names: Record<string, string> = {};
  for (const device of devices.value) {
    names[device.device_id] = device.name;
  }
  return names;
});

const thisDevice = computed(
  () => devices.value.find((device) => device.is_this_device) ?? null
);

const syncSetVaults = computed(() => syncSet.value?.vaults ?? []);

const visiblePeers = computed(() =>
  peers.value.filter((peer) => peer.device_id !== thisDevice.value?.device_id)
);

const pairedPeers = computed(() => devices.value.filter((device) => !device.is_this_device));

const activePeerLabel = computed(() => {
  if (!syncingPeerId.value) return null;
  return deviceNames.value[syncingPeerId.value] ?? `Device ${syncingPeerId.value.slice(0, 8)}`;
});

const latestLastSync = computed<string | null>(() => {
  const times = devices.value
    .map((device) => device.last_sync_at)
    .filter((value): value is string => !!value)
    .map((value) => new Date(value).getTime())
    .filter((value) => Number.isFinite(value));

  if (times.length === 0) return null;
  return new Date(Math.max(...times)).toISOString();
});

function deviceFor(peer: DiscoveredPeer): PairedDevice | undefined {
  return devices.value.find((device) => device.device_id === peer.device_id);
}

function peerLabel(peer: DiscoveredPeer): string {
  const device = deviceFor(peer);
  return device?.name ?? `Unknown Device ${shortId(peer.device_id)}`;
}

function peerStatus(peer: DiscoveredPeer): 'this' | 'unknown' | 'revoked' | 'paired' {
  const device = deviceFor(peer);
  if (device?.is_this_device) return 'this';
  if (!peer.paired) return 'unknown';
  if (device?.revoked) return 'revoked';
  return 'paired';
}

function lastSyncLabel(device: PairedDevice): string {
  if (device.is_this_device) return 'This Device';
  return device.last_sync_at
    ? `Last synced ${formatTimestamp(device.last_sync_at)}`
    : 'Never synced with this Device';
}

async function refreshDevices() {
  loadingDevices.value = true;
  const listed = await listPairedDevices();
  if (listed) devices.value = listed;
  loadingDevices.value = false;
}

async function refreshPeers() {
  if (isSyncing.value) return;

  loadingPeers.value = true;
  const listed = await listDiscoveredPeers();
  loadingPeers.value = false;

  if (listed) {
    peers.value = listed;
    discoveryError.value = null;
  } else {
    discoveryError.value = error.value ?? 'Devices could not be discovered.';
  }
}

async function refreshSyncSet() {
  const set = await getSyncSet();
  if (set) syncSet.value = set;
}

async function refreshUnregistered() {
  const listed = await listUnregisteredVaults();
  if (listed) unregistered.value = listed;
}

async function refreshAll() {
  await Promise.all([refreshDevices(), refreshSyncSet(), refreshUnregistered()]);
}

async function beginDiscovery() {
  // A previous screen's session may still be tearing down. Starting from a
  // known-stopped state keeps a quick navigate-away-and-back deterministic.
  await stopDiscovery();

  const started = await startDiscovery();
  discoveryOn.value = started;

  if (!started) {
    discoveryError.value = error.value ?? 'Devices could not be discovered.';
    return;
  }

  // Serving needs an open discovery session to have a listener; starting it
  // never advertises on its own.
  await startServing();

  await refreshPeers();
}

async function syncNow(device: PairedDevice) {
  navigationBlocked.value = null;
  lastAttemptPeerId.value = device.device_id;

  const report = await syncWithDevice(device.device_id);
  await refreshDevices();

  if (report && report.conflicts.length === 0) {
    await Promise.all([refreshSyncSet(), refreshUnregistered()]);
    duplicateReview.value?.refresh();
  }
}

function retryLastSync() {
  const peerId = lastAttemptPeerId.value;
  const device = peerId ? devices.value.find((candidate) => candidate.device_id === peerId) : null;
  if (device) {
    void syncNow(device);
  }
}

async function cancelRunningSync() {
  await cancelSync();
}

async function handleResolveConflict(conflict: SyncConflict, choice: ConflictChoice) {
  const session = conflicts.value;
  if (!session) return;

  await resolveSyncConflict({
    sessionId: session.sessionId,
    vaultId: conflict.vault_id,
    itemId: conflict.item_id ?? null,
    fieldKey: conflict.field_key,
    choice
  });
}

function abandonOpenConflicts() {
  abandonConflicts();
  navigationBlocked.value = null;
  void Promise.all([refreshSyncSet(), refreshUnregistered()]).then(() => {
    duplicateReview.value?.refresh();
  });
}

function openPairing(initialMode: 'source' | 'join') {
  openModal(markRaw(PairingModal), {
    initialMode,
    onClose: () => {
      void refreshAll();
    }
  });
}

function confirmRevoke(device: PairedDevice) {
  const label = device.name.trim() || `Device ${shortId(device.device_id)}`;
  openModal(markRaw(AlertModal), {
    title: `Revoke “${label}”?`,
    message:
      'Revocation is permanent and travels to every Device on the next sync. No honest Device will sync with it again. It does not take back anything that Device already holds, and it cannot be undone by that Device.',
    actionLabel: 'Revoke Device',
    confirmationValue: label,
    onClose: () => {},
    onConfirm: async () => {
      actionError.value = null;
      const revoked = await revokeDevice(device.device_id);
      if (revoked) {
        await refreshDevices();
      } else {
        actionError.value = error.value ?? 'The Device could not be revoked.';
      }
    }
  });
}

function recoverIdentity() {
  openModal(markRaw(AlertModal), {
    title: 'Start with a new identity?',
    message:
      'This is one-way. This installation becomes a new Device, other Devices will not recognise it, and the old Device’s entry is not revoked. Revoke the old Device from another Device if it was lost.',
    actionLabel: 'Start with a new identity',
    confirmationValue: 'new identity',
    onClose: () => {},
    onConfirm: async () => {
      const recovered = await startWithNewDeviceIdentity(true);
      if (recovered) {
        // The session and every manager were rebuilt under the new identity.
        // Reload so the app starts from a clean signed-out state.
        window.location.reload();
      }
    }
  });
}

async function handlePeerAction(peer: DiscoveredPeer) {
  const device = deviceFor(peer);
  if (device && peerStatus(peer) === 'paired') {
    await syncNow(device);
    return;
  }

  openPairing('source');
}

watch(
  () => conflicts.value,
  (next, previous) => {
    if (!next && previous) {
      navigationBlocked.value = null;
      void Promise.all([refreshSyncSet(), refreshUnregistered()]).then(() => {
        duplicateReview.value?.refresh();
      });
    }
  }
);

watch(
  () => isSyncing.value,
  (running) => {
    if (!running) navigationBlocked.value = null;
  }
);

onMounted(async () => {
  unlistenProgress = await onSyncProgress((payload) => {
    progress.value.push(payload);
  });

  identityError.value = await getDeviceIdentityError();
  await refreshAll();
  await beginDiscovery();

  pollTimer = setInterval(() => {
    void refreshPeers();
  }, 3000);
});

onUnmounted(() => {
  if (pollTimer) clearInterval(pollTimer);
  unlistenProgress?.();
  if (discoveryOn.value) {
    void stopServing();
    void stopDiscovery();
  }
});

onBeforeRouteLeave(() => {
  if (isSyncing.value) {
    navigationBlocked.value =
      'A sync is running. Wait for it to finish, or cancel it, before leaving this screen.';
    return false;
  }

  if (conflicts.value) {
    navigationBlocked.value =
      'A conflict is still open. Resolve it, or choose “Leave unresolved”, before leaving this screen.';
    return false;
  }

  return true;
});
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
        <section v-if="identityError" class="identity-banner">
          <TriangleAlert :size="22" aria-hidden="true" />
          <div>
            <strong>This Device’s identity is damaged</strong>
            <p>{{ identityError }}</p>
          </div>
          <Button variant="danger" size="small" @click="recoverIdentity">
            Start with a new identity
          </Button>
        </section>

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

        <div class="section-heading">
          <div>
            <h2>Paired Devices</h2>
            <p class="section-hint">
              Sync happens explicitly, one Device at a time. Discovery alone never grants access.
            </p>
          </div>
          <div class="pairing-actions">
            <Button variant="accent" size="small" @click="openPairing('source')">
              Pair a new Device
            </Button>
            <Button variant="outline" size="small" @click="openPairing('join')">
              Join a Device
            </Button>
          </div>
        </div>

        <p v-if="loadingDevices && devices.length === 0" class="muted">Loading Devices…</p>
        <p v-else-if="pairedPeers.length === 0" class="empty-state">
          No other Devices are paired yet. Pair one to start syncing your Vaults.
        </p>

        <ul v-else class="device-list">
          <li
            v-for="device in pairedPeers"
            :key="device.device_id"
            class="device-card"
            :class="{ 'device-card--revoked': device.revoked }"
          >
            <div class="device-icon">
              <Laptop v-if="device.platform.toLowerCase().includes('mac') || device.platform.toLowerCase().includes('linux') || device.platform.toLowerCase().includes('windows')" :size="20" aria-hidden="true" />
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
                :disabled="isSyncing"
                @click="syncNow(device)"
              >
                Sync now
              </Button>
              <Button
                v-if="!device.revoked"
                variant="label"
                size="small"
                :icon-component="ShieldOff"
                :disabled="isSyncing"
                @click="confirmRevoke(device)"
              >
                Revoke
              </Button>
            </div>
          </li>
        </ul>

        <section class="discovery-section">
          <header class="section-heading">
            <div>
              <h2 class="discovery-title">
                <Wifi v-if="!discoveryError" :size="18" aria-hidden="true" />
                <WifiOff v-else :size="18" aria-hidden="true" />
                On this network
              </h2>
              <p class="section-hint">
                Discovery stops when you leave this screen. A discovered Device is not syncable
                until it is paired.
              </p>
            </div>
            <Button
              variant="outline"
              size="small"
              :icon-component="RefreshCw"
              :disabled="loadingPeers || isSyncing"
              @click="refreshPeers"
            >
              Refresh
            </Button>
          </header>

          <div v-if="discoveryError" class="discovery-error">
            <WifiOff :size="18" aria-hidden="true" />
            <div>
              <strong>Discovery is unavailable</strong>
              <p>{{ discoveryError }}</p>
            </div>
          </div>

          <p v-else-if="visiblePeers.length === 0" class="empty-state">
            No other Devices are visible right now. Make sure the other Device has Lockr open on
            the Devices screen.
          </p>

          <ul v-else class="peer-list">
            <li v-for="peer in visiblePeers" :key="peer.device_id" class="peer-card">
              <div class="device-icon device-icon--peer">
                <Wifi :size="18" aria-hidden="true" />
              </div>

              <div class="device-info">
                <div class="device-name-row">
                  <strong>{{ peerLabel(peer) }}</strong>
                  <span v-if="peerStatus(peer) === 'paired'" class="tag tag--paired">Paired</span>
                  <span v-else-if="peerStatus(peer) === 'revoked'" class="tag tag--danger">
                    Revoked
                  </span>
                  <span v-else class="tag tag--unknown">Unknown</span>
                </div>
                <p class="device-platform">{{ peer.address }}</p>
                <p v-if="peerStatus(peer) === 'unknown'" class="device-last-sync">
                  Not paired. Seeing it grants no access.
                </p>
                <p v-else-if="peerStatus(peer) === 'revoked'" class="device-last-sync">
                  Revoked Devices cannot be synced with.
                </p>
              </div>

              <div class="device-actions">
                <Button
                  v-if="peerStatus(peer) === 'paired'"
                  variant="accent"
                  size="small"
                  :disabled="isSyncing"
                  @click="handlePeerAction(peer)"
                >
                  Sync now
                </Button>
                <Button
                  v-else-if="peerStatus(peer) === 'unknown'"
                  variant="outline"
                  size="small"
                  :disabled="isSyncing"
                  @click="handlePeerAction(peer)"
                >
                  Pair…
                </Button>
                <span v-else class="tag tag--danger">No sync</span>
              </div>
            </li>
          </ul>
        </section>

        <DuplicateReview
          ref="duplicateReview"
          :vaults="syncSetVaults"
          :notes="duplicateNotes"
        />

        <SyncSetEditor
          :vaults="syncSetVaults"
          :unregistered="unregistered"
          @changed="refreshAll"
        />

        <section v-if="thisDevice" class="this-device-note">
          <ShieldCheck :size="16" aria-hidden="true" />
          <p>
            This Device is <strong>{{ thisDevice.name }}</strong> ({{ thisDevice.platform }}).
            Its identity is a fresh id per installation; a reinstall joins as a new Device.
          </p>
        </section>
      </div>
    </div>

    <ConflictPrompt
      v-if="conflicts"
      :session="conflicts"
      :vault-names="vaultNames"
      :device-names="deviceNames"
      :this-device-id="thisDevice?.device_id ?? null"
      :busy="isResolvingConflict"
      :error="conflictError"
      @resolve="handleResolveConflict"
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

.identity-banner,
.navigation-blocked {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.875rem 1rem;
  border-radius: 0.625rem;
  font-size: 0.8125rem;
  line-height: 1.5;
}

.identity-banner {
  background: hsl(0 70% 97%);
  border: 1px solid hsl(0 70% 85%);
  color: hsl(0 60% 30%);

  > div {
    flex: 1;
  }

  p {
    margin-top: 0.25rem;
    overflow-wrap: anywhere;
  }
}

.navigation-blocked {
  background: hsl(38 90% 96%);
  border: 1px solid hsl(38 80% 82%);
  color: hsl(30 60% 28%);
  font-size: 0.8125rem;
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

.section-heading {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;

  h2 {
    font-family: var(--font-geo);
    font-size: 1.125rem;
    font-weight: 500;
  }
}

.section-hint {
  margin-top: 0.25rem;
  color: var(--color-text-muted);
  font-size: 0.8125rem;
  line-height: 1.4;
}

.pairing-actions {
  display: flex;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.device-list,
.peer-list {
  display: flex;
  flex-direction: column;
  gap: 0.5rem;
  list-style: none;
}

.device-card,
.peer-card {
  display: flex;
  align-items: center;
  gap: 0.875rem;
  padding: 0.875rem 1rem;
  border: 1px solid var(--color-border);
  border-radius: 0.625rem;
}

.device-card--revoked {
  opacity: 0.72;
  background: var(--color-bg-nav);
}

.device-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 2.5rem;
  height: 2.5rem;
  flex-shrink: 0;
  border-radius: 0.5rem;
  background: var(--color-accent-hover);
  color: var(--color-accent);
}

.device-icon--peer {
  background: var(--color-hover);
  color: var(--color-text-muted);
}

.device-info {
  flex: 1;
  min-width: 0;
}

.device-name-row {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.device-platform,
.device-last-sync {
  margin-top: 0.125rem;
  color: var(--color-text-muted);
  font-size: 0.75rem;
  overflow-wrap: anywhere;
}

.device-actions {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  flex-wrap: wrap;
  justify-content: flex-end;
}

.tag {
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
  font-size: 0.6875rem;
  white-space: nowrap;
}

.tag--paired {
  background: var(--color-accent-hover);
  color: var(--color-accent);
}

.tag--danger {
  background: hsl(0 70% 96%);
  color: var(--color-red);
}

.tag--unknown {
  background: var(--color-hover);
  color: var(--color-text-muted);
}

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

.this-device-note {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
  color: var(--color-text-muted);
  font-size: 0.75rem;
  line-height: 1.5;
}

.empty-state {
  padding: 1rem;
  border: 1px dashed var(--color-border);
  border-radius: 0.5rem;
  color: var(--color-text-muted);
  font-size: 0.875rem;
  line-height: 1.5;
}

.muted {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
}
</style>
