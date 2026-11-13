<script setup lang="ts">
import { computed } from 'vue';
import { TriangleAlert, LoaderCircle, RotateCw, ShieldAlert, X } from '@lucide/vue';
import Button from '../ui/Button.vue';
import type { SyncProgress } from '../../types';
import type { SyncFailure } from '../../composables/useSync';
import { formatTimestamp } from '../../util/timestamp';

const props = defineProps<{
  syncing: boolean;
  peerLabel: string | null;
  progress: SyncProgress[];
  failure: SyncFailure | null;
  lastSyncedAt: string | null;
  vaultNames: Record<string, string>;
}>();

const emit = defineEmits<{
  (e: 'cancel'): void;
  (e: 'retry'): void;
  (e: 'dismiss'): void;
  (e: 'recover-identity'): void;
}>();

const totalVaults = computed(() => {
  for (let index = props.progress.length - 1; index >= 0; index -= 1) {
    const event = props.progress[index];
    if (event.event === 'started' || event.event === 'scope_agreed') {
      return event.vault_ids.length;
    }
  }
  return 0;
});

const finishedVaults = computed(() => {
  const finished = new Set<string>();
  for (const event of props.progress) {
    if (event.event === 'vault_merged' || event.event === 'vault_conflicted') {
      finished.add(event.vault_id);
    }
  }
  return finished.size;
});

const percent = computed(() => {
  if (totalVaults.value === 0) return 0;
  return Math.min(100, Math.round((finishedVaults.value / totalVaults.value) * 100));
});

const completeEvent = computed(() => {
  for (let index = props.progress.length - 1; index >= 0; index -= 1) {
    const event = props.progress[index];
    if (event.event === 'complete') return event;
  }
  return null;
});

const activity = computed(() => {
  for (let index = props.progress.length - 1; index >= 0; index -= 1) {
    const event = props.progress[index];
    const vaultName = 'vault_id' in event ? props.vaultNames[event.vault_id] : undefined;

    switch (event.event) {
      case 'started':
        return 'Opening the secure channel…';
      case 'scope_agreed':
        return `Exchanging ${event.vault_ids.length} Vault${event.vault_ids.length === 1 ? '' : 's'}…`;
      case 'sending_vault':
        return `Sending ${vaultName ?? 'a Vault'}…`;
      case 'receiving_vault':
        return `Receiving ${vaultName ?? 'a Vault'}…`;
      case 'vault_merged':
        return `Merged ${vaultName ?? 'a Vault'}`;
      case 'vault_conflicted':
        return `${event.conflicts} Conflict${event.conflicts === 1 ? '' : 's'} in ${vaultName ?? 'a Vault'}`;
      case 'complete':
        return 'Sync complete';
    }
  }
  return 'Starting…';
});

const lastSyncedLabel = computed(() =>
  props.lastSyncedAt ? formatTimestamp(props.lastSyncedAt) : null
);
</script>

<template>
  <section class="sync-panel" aria-live="polite">
    <header class="panel-header">
      <div>
        <h2>Sync</h2>
        <p class="panel-hint">
          Sync only runs when you ask. Your master password never leaves this Device.
        </p>
      </div>

      <span v-if="syncing" class="status-badge status-badge--active">
        <LoaderCircle :size="14" class="spin" aria-hidden="true" />
        Syncing
      </span>
      <span v-else-if="failure" class="status-badge status-badge--error">
        <TriangleAlert :size="14" aria-hidden="true" />
        Did not complete
      </span>
    </header>

    <div v-if="syncing" class="sync-running">
      <div class="sync-heading">
        <strong>{{ peerLabel ?? 'Another Device' }}</strong>
        <span v-if="totalVaults > 0">{{ finishedVaults }} of {{ totalVaults }}</span>
      </div>

      <div
        class="progress-track"
        role="progressbar"
        :aria-valuenow="percent"
        aria-valuemin="0"
        aria-valuemax="100"
        :aria-label="activity"
      >
        <span
          class="progress-fill"
          :class="{ 'progress-fill--indeterminate': totalVaults === 0 }"
          :style="totalVaults > 0 ? { width: `${percent}%` } : undefined"
        ></span>
      </div>

      <div class="sync-footer">
        <span class="activity">{{ activity }}</span>
        <Button variant="outline" size="small" :icon-component="X" @click="emit('cancel')">
          Cancel sync
        </Button>
      </div>
    </div>

    <div v-else-if="failure" class="sync-failure" :class="`sync-failure--${failure.kind}`">
      <div class="failure-title">
        <ShieldAlert v-if="failure.kind === 'revoked' || failure.kind === 'this-device-revoked'" :size="20" aria-hidden="true" />
        <TriangleAlert v-else :size="20" aria-hidden="true" />
        <strong>{{ failure.title }}</strong>
      </div>
      <p class="failure-advice">{{ failure.advice }}</p>
      <details class="failure-detail">
        <summary>Technical detail</summary>
        <code>{{ failure.detail }}</code>
      </details>

      <div class="failure-actions">
        <Button
          v-if="failure.canRetry"
          variant="outline"
          size="small"
          :icon-component="RotateCw"
          @click="emit('retry')"
        >
          Try again
        </Button>
        <Button
          v-if="failure.canRecoverIdentity"
          variant="danger"
          size="small"
          @click="emit('recover-identity')"
        >
          Start with a new identity
        </Button>
        <Button variant="label" size="small" @click="emit('dismiss')">Dismiss</Button>
      </div>
    </div>

    <div v-else-if="completeEvent" class="sync-complete">
      <p>
        <strong>Sync complete.</strong>
        {{ completeEvent.vaults }} Vault{{ completeEvent.vaults === 1 ? '' : 's' }} exchanged,
        {{ completeEvent.conflicts }} Conflict{{ completeEvent.conflicts === 1 ? '' : 's' }} found.
      </p>
      <p v-if="lastSyncedLabel" class="last-synced">Last synced {{ lastSyncedLabel }}</p>
    </div>

    <div v-else class="sync-idle">
      <p v-if="lastSyncedLabel" class="last-synced">Last synced {{ lastSyncedLabel }}</p>
      <p v-else class="last-synced">This Device has not completed a sync yet.</p>
      <p class="panel-hint">Choose a paired Device below to sync with it.</p>
    </div>
  </section>
</template>

<style scoped>
.sync-panel {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding: 1.25rem;
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
  background: var(--color-bg-nav);
}

.panel-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;

  > div {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
  }

  h2 {
    font-family: var(--font-geo);
    font-size: 1.125rem;
    font-weight: 500;
  }
}

.panel-hint {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
  line-height: 1.4;
}

.status-badge {
  display: inline-flex;
  align-items: center;
  gap: 0.375rem;
  padding: 0.25rem 0.625rem;
  border-radius: 999px;
  font-size: 0.75rem;
  white-space: nowrap;
}

.status-badge--active {
  color: var(--color-accent);
  background: var(--color-accent-hover);
}

.status-badge--error {
  color: var(--color-red);
  background: hsl(0 70% 96%);
}

.sync-running,
.sync-complete,
.sync-idle {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
}

.sync-heading,
.sync-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  font-size: 0.875rem;
}

.activity {
  color: var(--color-text-secondary);
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.progress-track {
  position: relative;
  width: 100%;
  height: 0.5rem;
  overflow: hidden;
  border-radius: 999px;
  background: var(--color-hover);
  box-shadow: var(--inset-sm);
}

.progress-fill {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: var(--color-accent);
  transition: width 0.25s ease;
}

.progress-fill--indeterminate {
  width: 40%;
  animation: indeterminate 1.2s ease-in-out infinite;
}

@keyframes indeterminate {
  from {
    transform: translateX(-100%);
  }
  to {
    transform: translateX(250%);
  }
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.sync-failure {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0.875rem;
  border-radius: 0.5rem;
  background: hsl(0 70% 97%);
  border: 1px solid hsl(0 70% 88%);
}

.failure-title {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  color: var(--color-red);
}

.failure-advice {
  color: var(--color-text-secondary);
  font-size: 0.875rem;
  line-height: 1.5;
}

.failure-detail {
  font-size: 0.75rem;
  color: var(--color-text-muted);

  summary {
    cursor: pointer;
  }

  code {
    display: block;
    margin-top: 0.375rem;
    padding: 0.5rem;
    overflow-wrap: anywhere;
    background: var(--color-bg);
    border-radius: 0.375rem;
  }
}

.failure-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
}

.last-synced {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
}
</style>
