<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { TriangleAlert, ChevronLeft, ChevronRight, ShieldQuestionMark } from '@lucide/vue';
import Button from '../ui/Button.vue';
import ConflictSideCard from './ConflictSideCard.vue';
import type { ConflictChoice, SyncConflict, SyncConflictSide } from '../../types';
import { shortId, vaultLabelById } from '../../util/labels';
import { conflictFieldLabel } from '../../util/conflictLabels';
import {
  clearReveals,
  conflictSideKey,
  hideSide,
  isSideRevealed,
  revealSide,
  type ConflictSideEntry,
  type RevealedConflictValues
} from '../../util/conflictRedaction';
import type { ConflictSession } from '../../composables/useSync';
import { useSync } from '../../composables/useSync';

const props = defineProps<{
  session: ConflictSession;
  vaultNames: Record<string, string>;
  deviceNames: Record<string, string>;
  busy: boolean;
  error: string | null;
}>();

const emit = defineEmits<{
  (e: 'resolve', conflict: SyncConflict, choice: ConflictChoice): void;
  (e: 'resync', peerDeviceId: string): void;
  (e: 'abandon'): void;
}>();

const index = ref(0);
const { revealConflictCandidate } = useSync();
const revealed = ref<RevealedConflictValues>({});
const revealing = ref<string | null>(null);

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

// A revealed value belongs to the field the User was looking at. Moving to
// another Conflict re-masks it rather than carrying a secret along.
watch(current, () => {
  revealed.value = clearReveals();
  revealing.value = null;
});

function vaultLabel(vaultId: string): string {
  return vaultLabelById(vaultId, props.vaultNames);
}

function deviceLabel(side: SyncConflictSide): string {
  if (side.is_local) return 'This Device';
  return props.deviceNames[side.device] ?? `Device ${shortId(side.device)}`;
}

const itemLabel = computed(() => {
  const conflict = current.value;
  if (!conflict) return '';
  if (conflict.scope === 'vault') return vaultLabel(conflict.vault_id);
  return conflict.display_name?.trim() || `Item ${shortId(conflict.item_id ?? '')}`;
});

/**
 * The backend marks the side this Device wrote, if any. At most one side is
 * local. When neither is, the conflict arrived wholly from other Devices and
 * the prompt must not offer a blind choice between values it cannot identify.
 */
const hasLocalSide = computed(
  () => current.value?.sides.some((side) => side.is_local) ?? false
);

const sideEntries = computed<ConflictSideEntry[]>(() => {
  const conflict = current.value;
  if (!conflict) return [];

  const [first, second] = conflict.sides;

  if (!hasLocalSide.value) {
    return [
      { choice: null, side: first, label: deviceLabel(first) },
      { choice: null, side: second, label: deviceLabel(second) }
    ];
  }

  const local = first.is_local ? first : second;
  const remote = first.is_local ? second : first;

  return [
    { choice: 'local', side: local, label: deviceLabel(local) },
    { choice: 'remote', side: remote, label: deviceLabel(remote) }
  ];
});

function sideKey(entry: ConflictSideEntry): string {
  return conflictSideKey(entry.side);
}

function isRevealed(entry: ConflictSideEntry): boolean {
  return isSideRevealed(revealed.value, entry.side);
}

async function toggleReveal(entry: ConflictSideEntry) {
  if (!current.value || !entry.choice || props.busy) return;

  const key = sideKey(entry);

  if (isRevealed(entry)) {
    revealed.value = hideSide(revealed.value, entry.side);
    return;
  }

  revealing.value = key;
  const result = await revealConflictCandidate({
    sessionId: props.session.sessionId,
    vaultId: current.value.vault_id,
    itemId: current.value.item_id ?? null,
    fieldKey: current.value.field_key,
    choice: entry.choice
  });
  revealing.value = null;

  if (result) {
    revealed.value = revealSide(revealed.value, entry.side, result.value);
  }
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
              <dd>{{ conflictFieldLabel(current.field_key) }}</dd>
            </div>
          </dl>

          <p v-if="current.is_secret" class="secret-note">
            <TriangleAlert :size="16" aria-hidden="true" />
            This is a secret field. Lockr does not put either value in the Conflict report; you can
            reveal one side at a time when you need to compare them. Choosing a side copies that
            Device's value without revealing it.
          </p>

          <div class="side-grid">
            <ConflictSideCard
              v-for="entry in sideEntries"
              :key="`${entry.side.device}:${entry.side.counter}`"
              :entry="entry"
              :is-secret="current.is_secret"
              :revealed="isRevealed(entry)"
              :revealed-value="revealed[sideKey(entry)]"
              :busy="busy"
              :revealing="revealing === sideKey(entry)"
              @reveal="toggleReveal(entry)"
              @choose="choose"
            />
          </div>

          <div v-if="!hasLocalSide" class="ambiguity-note">
            <TriangleAlert :size="16" aria-hidden="true" />
            <div>
              <p>This conflict arrived from other Devices; sync again to refresh.</p>
              <Button
                variant="outline"
                size="small"
                :disabled="busy"
                @click="emit('resync', session.peerDeviceId)"
              >
                Sync again
              </Button>
            </div>
          </div>

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

<style scoped src="./ConflictPrompt.css"></style>
