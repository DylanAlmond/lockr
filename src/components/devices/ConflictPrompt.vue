<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { TriangleAlert, ChevronLeft, ChevronRight, ShieldQuestionMark } from '@lucide/vue';
import Button from '../ui/Button.vue';
import type { ConflictChoice, SyncConflict, SyncConflictSide } from '../../types';
import type { ConflictSession } from '../../composables/useSync';

const props = defineProps<{
  session: ConflictSession;
  vaultNames: Record<string, string>;
  deviceNames: Record<string, string>;
  thisDeviceId: string | null;
  busy: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (e: 'resolve', conflict: SyncConflict, choice: ConflictChoice): void;
  (e: 'abandon'): void;
}>();

const index = ref(0);

watch(
  () => props.session.conflicts.length,
  (length) => {
    if (length === 0) {
      index.value = 0;
    } else if (index.value >= length) {
      index.value = length - 1;
    }
  }
);

const conflicts = computed(() => props.session.conflicts);
const current = computed<SyncConflict | null>(() => conflicts.value[index.value] ?? null);
const position = computed(() => `${index.value + 1} of ${conflicts.value.length}`);

const FIELD_LABELS: Record<string, string> = {
  display_name: 'Display name',
  username: 'Username',
  email: 'Email address',
  phone: 'Phone number',
  full_name: 'Full name',
  address_line1: 'Address line 1',
  address_line2: 'Address line 2',
  city: 'City',
  postal_code: 'Postal code',
  country: 'Country',
  cardholder_name: 'Cardholder name',
  brand: 'Card brand',
  expiry_month: 'Expiry month',
  expiry_year: 'Expiry year',
  title: 'Title',
  body: 'Note body',
  favourite: 'Favourite',
  tags: 'Tags',
  icon: 'Icon',
  color: 'Colour',
  name: 'Name'
};

function fieldLabel(fieldKey: string): string {
  return (
    FIELD_LABELS[fieldKey] ??
    fieldKey
      .replace(/_/g, ' ')
      .replace(/^./, (letter) => letter.toUpperCase())
  );
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

function vaultLabel(vaultId: string): string {
  return props.vaultNames[vaultId] ?? `Vault ${shortId(vaultId)}`;
}

function deviceLabel(deviceId: string): string {
  if (deviceId === props.thisDeviceId) return 'This Device';
  return props.deviceNames[deviceId] ?? `Device ${shortId(deviceId)}`;
}

const itemLabel = computed(() => {
  const conflict = current.value;
  if (!conflict) return '';
  if (conflict.scope === 'vault') return vaultLabel(conflict.vault_id);
  return conflict.display_name?.trim() || `Item ${shortId(conflict.item_id ?? '')}`;
});

interface SideEntry {
  choice: ConflictChoice | null;
  side: SyncConflictSide;
  label: string;
}

/**
 * The backend orders Conflict sides canonically, not local-first, and does not
 * mark which side is this Device's copy. When one side names this Device we can
 * map it safely; when neither does, we must not guess which hidden value is
 * local, so the prompt withholds both choices.
 */
const localSideKnown = computed(() => {
  const conflict = current.value;
  if (!conflict || !props.thisDeviceId) return false;
  return conflict.sides.some((side) => side.device === props.thisDeviceId);
});

const sideEntries = computed<SideEntry[]>(() => {
  const conflict = current.value;
  if (!conflict) return [];

  const [first, second] = conflict.sides;

  if (!localSideKnown.value) {
    return [
      { choice: null, side: first, label: deviceLabel(first.device) },
      { choice: null, side: second, label: deviceLabel(second.device) }
    ];
  }

  let local = first;
  let remote = second;

  if (second.device === props.thisDeviceId) {
    local = second;
    remote = first;
  }

  return [
    { choice: 'local', side: local, label: deviceLabel(local.device) },
    { choice: 'remote', side: remote, label: deviceLabel(remote.device) }
  ];
});

function formatValue(value: unknown): string {
  if (value === null || value === undefined) return 'Not set';

  if (typeof value === 'string') {
    return value.length > 0 ? value : 'Empty';
  }

  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  if (typeof value === 'number') return String(value);

  if (Array.isArray(value)) {
    return value.length > 0 ? value.join(', ') : 'None';
  }

  return JSON.stringify(value);
}

function sideValue(side: SyncConflictSide): string {
  if (current.value?.is_secret) return 'Value hidden';
  return formatValue(side.value);
}

function choose(choice: ConflictChoice | null) {
  if (!current.value || props.busy || !choice) return;
  emit('resolve', current.value, choice);
}

function previous() {
  if (index.value > 0) index.value -= 1;
}

function next() {
  if (index.value < conflicts.value.length - 1) index.value += 1;
}
</script>

<template>
  <Teleport to="body">
    <div class="conflict-backdrop">
      <article
        class="conflict-card thin-scrollbar"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="conflict-title"
        @keydown.esc.prevent
      >
        <header class="conflict-header">
          <div class="conflict-heading">
            <ShieldQuestionMark :size="22" aria-hidden="true" />
            <h2 id="conflict-title">Resolve a conflict</h2>
          </div>
          <span class="conflict-position">{{ position }}</span>
        </header>

        <p class="conflict-explainer">
          Two Devices changed the same field independently, so neither value can win automatically.
          Everything that merged cleanly is already saved. This sync stays open until every field
          is resolved.
        </p>

        <div v-if="current" class="conflict-body">
          <dl class="conflict-meta">
            <div>
              <dt>Vault</dt>
              <dd>{{ vaultLabel(current.vault_id) }}</dd>
            </div>
            <div>
              <dt>{{ current.scope === 'vault' ? 'Record' : 'Item' }}</dt>
              <dd>{{ itemLabel }}</dd>
            </div>
            <div>
              <dt>Field</dt>
              <dd>{{ fieldLabel(current.field_key) }}</dd>
            </div>
          </dl>

          <p v-if="current.is_secret" class="secret-note">
            <TriangleAlert :size="16" aria-hidden="true" />
            This is a secret field. Lockr never shows either value here; choosing a side copies
            that Device's value without revealing it.
          </p>

          <div class="side-grid">
            <section
              v-for="entry in sideEntries"
              :key="`${entry.side.device}:${entry.side.counter}`"
              class="conflict-side"
              :class="{ 'conflict-side--local': entry.choice === 'local' }"
            >
              <header>
                <span class="side-device">{{ entry.label }}</span>
                <span v-if="entry.choice === 'local'" class="side-tag">This copy</span>
              </header>

              <p class="side-value" :class="{ 'side-value--secret': current.is_secret }">
                {{ sideValue(entry.side) }}
              </p>

              <Button
                v-if="entry.choice"
                :variant="entry.choice === 'local' ? 'accent' : 'outline'"
                size="small"
                :disabled="busy"
                @click="choose(entry.choice)"
              >
                {{ current.is_secret ? 'Keep this secret value' : 'Keep this value' }}
              </Button>
            </section>
          </div>

          <p v-if="!localSideKnown" class="ambiguity-note">
            <TriangleAlert :size="16" aria-hidden="true" />
            Neither value was written by this Device, so Lockr cannot tell which side is this
            Device’s copy. Choosing here could keep the wrong hidden value. This conflict cannot be
            resolved from this screen; leave it unresolved and resolve it from one of the Devices
            named above.
          </p>

          <p v-if="error" class="conflict-error">{{ error }}</p>
        </div>

        <footer class="conflict-footer">
          <div v-if="conflicts.length > 1" class="pager">
            <Button
              variant="label"
              size="small"
              :icon-component="ChevronLeft"
              :disabled="index === 0"
              @click="previous"
            >
              Previous
            </Button>
            <Button
              variant="label"
              size="small"
              :icon-component="ChevronRight"
              :disabled="index >= conflicts.length - 1"
              @click="next"
            >
              Next
            </Button>
          </div>

          <Button variant="label" size="small" :disabled="busy" @click="emit('abandon')">
            Leave unresolved
          </Button>
        </footer>

        <p class="abandon-hint">
          Leaving unresolved keeps the clean merges and this Device's value. The sync does not
          finish, and the other side is not applied.
        </p>
      </article>
    </div>
  </Teleport>
</template>

<style scoped>
.conflict-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1100;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 1rem;
  background: rgb(0 0 0 / 0.35);
}

.conflict-card {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  box-sizing: border-box;
  width: min(42rem, 100%);
  max-height: calc(100% - 2rem);
  overflow-y: auto;
  padding: 1.5rem;
  background: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
  box-shadow: 0 16px 48px rgb(0 0 0 / 0.18);
}

.conflict-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
}

.conflict-heading {
  display: flex;
  align-items: center;
  gap: 0.625rem;
  color: var(--color-accent);

  h2 {
    font-family: var(--font-geo);
    font-size: 1.375rem;
    font-weight: 500;
    color: var(--color-text);
  }
}

.conflict-position {
  padding: 0.25rem 0.625rem;
  border-radius: 999px;
  background: var(--color-hover);
  color: var(--color-text-muted);
  font-size: 0.75rem;
  white-space: nowrap;
}

.conflict-explainer {
  color: var(--color-text-secondary);
  font-size: 0.875rem;
  line-height: 1.5;
}

.conflict-body {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.conflict-meta {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
  gap: 0.75rem;
  padding: 0.75rem;
  border-radius: 0.5rem;
  background: var(--color-bg-nav);

  dt {
    color: var(--color-text-muted);
    font-size: 0.6875rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  dd {
    font-size: 0.875rem;
    overflow-wrap: anywhere;
  }
}

.secret-note {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
  padding: 0.75rem;
  border-radius: 0.5rem;
  background: hsl(38 90% 96%);
  border: 1px solid hsl(38 80% 85%);
  color: hsl(30 60% 28%);
  font-size: 0.8125rem;
  line-height: 1.5;
}

.ambiguity-note {
  display: flex;
  align-items: flex-start;
  gap: 0.5rem;
  padding: 0.75rem;
  border-radius: 0.5rem;
  background: hsl(0 70% 97%);
  border: 1px solid hsl(0 70% 85%);
  color: hsl(0 60% 30%);
  font-size: 0.8125rem;
  line-height: 1.5;
}

.side-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
  gap: 0.75rem;
}

.conflict-side {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.875rem;
  border: 1px solid var(--color-border);
  border-radius: 0.5rem;

  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    flex-wrap: wrap;
  }
}

.conflict-side--local {
  border-color: var(--color-accent);
  box-shadow: inset 0 0 0 1px var(--color-accent);
}

.side-device {
  font-size: 0.8125rem;
  font-weight: 500;
}

.side-tag {
  padding: 0.125rem 0.5rem;
  border-radius: 999px;
  background: var(--color-accent-hover);
  color: var(--color-accent);
  font-size: 0.6875rem;
}

.side-value {
  flex: 1;
  width: 100%;
  min-height: 3rem;
  padding: 0.625rem;
  overflow-wrap: anywhere;
  white-space: pre-wrap;
  background: var(--color-bg-nav);
  border-radius: 0.375rem;
  font-size: 0.875rem;
  line-height: 1.45;
}

.side-value--secret {
  display: flex;
  align-items: center;
  color: var(--color-text-muted);
  font-style: italic;
}

.conflict-error {
  color: var(--color-red);
  font-size: 0.8125rem;
}

.conflict-footer {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 1rem;
  flex-wrap: wrap;
}

.pager {
  display: flex;
  gap: 0.25rem;
}

.abandon-hint {
  color: var(--color-text-muted);
  font-size: 0.75rem;
  line-height: 1.4;
}
</style>
