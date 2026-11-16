<script setup lang="ts">
import Button from '../ui/Button.vue';
import type { SyncSetVault } from '../../types';
import { vaultLabel } from '../../util/labels';

defineProps<{
  vault: SyncSetVault;
  busy: boolean;
}>();

const emit = defineEmits<{
  (e: 'stop-syncing'): void;
  (e: 'remove-local'): void;
  (e: 'delete-everywhere'): void;
}>();
</script>

<template>
  <article class="removal-vault">
    <header class="removal-vault-header">
      <strong>{{ vaultLabel(vault) }}</strong>
      <span class="sync-indicator" :class="{ 'sync-indicator--off': !vault.syncing }">
        {{ vault.syncing ? 'In the Sync Set' : 'Not syncing' }}
      </span>
    </header>

    <div class="removal-grid">
      <section class="removal-card removal-card--stop">
        <h4>Stop syncing</h4>
        <p>Keep the Vault here. It stops travelling and its copies may drift.</p>
        <Button
          variant="outline"
          size="xs"
          :disabled="busy || !vault.syncing"
          @click="emit('stop-syncing')"
        >
          Stop syncing
        </Button>
      </section>

      <section class="removal-card removal-card--remove">
        <h4>Remove from this Device</h4>
        <p>Delete this Device’s copy only. Other Devices keep theirs; no deletion travels.</p>
        <Button variant="outline" size="xs" :disabled="busy" @click="emit('remove-local')">
          Remove local copy
        </Button>
      </section>

      <section class="removal-card removal-card--delete">
        <h4>Delete everywhere</h4>
        <p>Record a deletion that travels to every Device. The copy remains only as trash.</p>
        <Button variant="danger" size="xs" :disabled="busy" @click="emit('delete-everywhere')">
          Delete everywhere
        </Button>
      </section>
    </div>
  </article>
</template>

<style scoped>
.removal-vault {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
  padding: 0.875rem;
  border: 1px solid var(--color-border);
  border-radius: 0.625rem;
}

.removal-vault-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  font-size: 0.875rem;
}

.removal-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
  gap: 0.625rem;
}

.removal-card {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.5rem;
  padding: 0.75rem;
  border-radius: 0.5rem;

  h4 {
    font-size: 0.8125rem;
    font-weight: 600;
  }

  p {
    flex: 1;
    font-size: 0.75rem;
    line-height: 1.4;
  }
}

.removal-card--stop {
  background: var(--color-bg-nav);
  border: 1px solid var(--color-border);
  color: var(--color-text-secondary);
}

.removal-card--remove {
  background: hsl(38 90% 97%);
  border: 1px solid hsl(38 80% 82%);
  color: hsl(30 55% 28%);
}

.removal-card--delete {
  background: hsl(0 70% 97%);
  border: 1px solid hsl(0 70% 85%);
  color: hsl(0 60% 32%);
}

.sync-indicator {
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
  background: var(--color-accent-hover);
  color: var(--color-accent);
  font-size: 0.6875rem;
  white-space: nowrap;
}

.sync-indicator--off {
  background: var(--color-hover);
  color: var(--color-text-muted);
}
</style>
