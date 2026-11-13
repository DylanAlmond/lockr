<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, shallowRef } from 'vue';
import { Check, Copy, KeyRound, LoaderCircle, QrCode, ShieldCheck } from '@lucide/vue';
import Button from '../ui/Button.vue';
import Input from '../ui/Input.vue';
import { useSync } from '../../composables/useSync';
import type {
  PairingApprovalPrompt,
  PairingMessage,
  PairingOffer,
  SyncSetVault
} from '../../types';

const props = withDefaults(
  defineProps<{
    initialMode?: 'source' | 'join';
  }>(),
  { initialMode: 'source' }
);

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const {
  error,
  startPairing,
  cancelPairing,
  joinPairing,
  receivePairingRequest,
  approvePairing,
  getSyncSet
} = useSync();

const mode = ref<'source' | 'join'>(props.initialMode);
const busy = ref(false);
const localError = ref<string | null>(null);

// Source Device
const offer = ref<PairingOffer | null>(null);
const vaults = ref<SyncSetVault[]>([]);
const syncSetLoaded = ref(false);
const checked = ref<Set<string>>(new Set());
const joinFrameText = ref('');
const approval = ref<PairingApprovalPrompt | null>(null);
// The transfer frame carries the User's secret key. It is held shallowly and
// only ever checked for presence; it is never rendered or copied to display.
const transfer = shallowRef<PairingMessage | null>(null);
const copied = ref(false);

// Joining Device
const codeMode = ref<'typed' | 'qr'>('typed');
const sourceDeviceId = ref('');
const typedCode = ref('');
const qrPayload = ref('');
const joinMessage = shallowRef<PairingMessage | null>(null);

const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;

onMounted(() => {
  timer = setInterval(() => {
    now.value = Date.now();
  }, 1000);
});

onUnmounted(() => {
  if (timer) clearInterval(timer);
  if (offer.value && !transfer.value) {
    void cancelPairing();
  }
  if (joinMessage.value) {
    void cancelPairing();
  }
});

const joinInfo = computed(() => {
  const message = joinMessage.value;
  if (!message || message.kind !== 'join') return null;
  return {
    device_id: message.device_id,
    name: message.name,
    platform: message.platform,
    schema_version: message.schema_version
  };
});

const secondsLeft = computed(() => {
  if (!offer.value) return 0;
  const expiry = new Date(offer.value.expires_at).getTime();
  return Math.max(0, Math.ceil((expiry - now.value) / 1000));
});

const offerExpired = computed(() => !!offer.value && secondsLeft.value <= 0);

function vaultLabel(vault: SyncSetVault): string {
  return vault.name?.trim() || `Vault ${vault.vault_id.slice(0, 8)}`;
}

function toggleVault(vaultId: string) {
  const next = new Set(checked.value);
  if (next.has(vaultId)) {
    next.delete(vaultId);
  } else {
    next.add(vaultId);
  }
  checked.value = next;
}

async function switchMode(next: 'source' | 'join') {
  if (mode.value === next) return;

  if (offer.value || joinMessage.value) {
    await cancelPairing();
  }

  mode.value = next;
  localError.value = null;
  offer.value = null;
  approval.value = null;
  transfer.value = null;
  joinMessage.value = null;
  joinFrameText.value = '';
  syncSetLoaded.value = false;
}

async function beginOffer() {
  busy.value = true;
  localError.value = null;
  transfer.value = null;
  approval.value = null;

  const created = await startPairing();
  busy.value = false;

  if (!created) {
    localError.value = error.value;
    return;
  }

  offer.value = created;

  const set = await getSyncSet();
  syncSetLoaded.value = !!set;
  vaults.value = set?.vaults ?? [];
  checked.value = new Set(vaults.value.map((vault) => vault.vault_id));
}

async function copyPayload() {
  if (!offer.value) return;
  try {
    await navigator.clipboard.writeText(offer.value.qr_payload);
    copied.value = true;
    setTimeout(() => {
      copied.value = false;
    }, 2000);
  } catch {
    localError.value = 'Could not copy the pairing payload.';
  }
}

async function reviewJoinFrame() {
  const text = joinFrameText.value.trim();
  if (!text) {
    localError.value = 'Paste the joining Device’s announcement first.';
    return;
  }

  let parsed: PairingMessage;
  try {
    parsed = JSON.parse(text) as PairingMessage;
  } catch {
    localError.value = 'That is not a pairing announcement.';
    return;
  }

  if (parsed.kind !== 'join') {
    localError.value = 'That frame is not a joining Device’s announcement.';
    return;
  }

  busy.value = true;
  localError.value = null;
  const prompt = await receivePairingRequest(parsed);
  busy.value = false;

  if (!prompt) {
    localError.value = error.value;
    return;
  }

  approval.value = prompt;
}

async function approve() {
  if (!approval.value) return;

  busy.value = true;
  localError.value = null;
  const frame = await approvePairing(
    approval.value.device_id,
    syncSetLoaded.value ? [...checked.value] : null
  );
  busy.value = false;

  if (!frame) {
    localError.value = error.value;
    return;
  }

  transfer.value = frame;
  offer.value = null;
}

async function beginJoin() {
  busy.value = true;
  localError.value = null;
  joinMessage.value = null;

  const code =
    codeMode.value === 'qr'
      ? ({ kind: 'qr', payload: qrPayload.value.trim() } as const)
      : ({
          kind: 'typed',
          source_device_id: sourceDeviceId.value.trim(),
          code: typedCode.value.trim()
        } as const);

  const message = await joinPairing(code);
  busy.value = false;

  if (!message) {
    localError.value = error.value;
    return;
  }

  joinMessage.value = message;
}

async function reset() {
  busy.value = true;
  await cancelPairing();
  busy.value = false;
  offer.value = null;
  approval.value = null;
  transfer.value = null;
  joinMessage.value = null;
  joinFrameText.value = '';
  qrPayload.value = '';
  typedCode.value = '';
  sourceDeviceId.value = '';
  localError.value = null;
  syncSetLoaded.value = false;
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

    <p v-if="localError" class="error-text">{{ localError }}</p>

    <!-- Source Device: offer, human approval, pre-checked Sync Set -->
    <section v-if="mode === 'source'" class="pairing-panel">
      <template v-if="!offer && !transfer">
        <p class="panel-copy">
          Start a single-use offer here, then read or scan it on the new Device. The code expires
          after about a minute and can be used once.
        </p>
        <Button variant="accent" :disabled="busy" @click="beginOffer">
          <LoaderCircle v-if="busy" :size="16" class="spin" aria-hidden="true" />
          Start pairing
        </Button>
      </template>

      <template v-else-if="offer">
        <div class="offer-card">
          <div class="offer-top">
            <span class="offer-label">Pairing code</span>
            <span class="countdown" :class="{ expired: offerExpired }">
              {{ offerExpired ? 'Expired' : `${secondsLeft}s left` }}
            </span>
          </div>

          <p class="typed-code">{{ offer.typed_code }}</p>
          <p class="panel-copy">
            Type this code on the new Device, or hand it the full payload below.
          </p>

          <details class="payload-details">
            <summary>
              <QrCode :size="15" aria-hidden="true" />
              QR payload
            </summary>
            <code class="payload">{{ offer.qr_payload }}</code>
            <Button variant="outline" size="xs" :icon-component="copied ? Check : Copy" @click="copyPayload">
              {{ copied ? 'Copied' : 'Copy payload' }}
            </Button>
          </details>
        </div>

        <div v-if="!approval" class="channel-step">
          <h3>Joining Device announcement</h3>
          <p class="panel-copy">
            A secure pairing channel normally carries this identity-only frame. Paste it here when
            your build provides one.
          </p>
          <textarea
            v-model="joinFrameText"
            class="frame-input thin-scrollbar"
            rows="4"
            spellcheck="false"
            placeholder='{"kind":"join","device_id":"…","name":"…","platform":"…","public_key":"…","schema_version":3}'
          ></textarea>
          <div class="panel-actions">
            <Button variant="outline" :disabled="busy || offerExpired" @click="reviewJoinFrame">
              Review Device
            </Button>
            <Button variant="label" :disabled="busy" @click="reset">Cancel pairing</Button>
          </div>
        </div>

        <div v-else class="approval-step">
          <div class="approval-card">
            <header>
              <ShieldCheck :size="20" aria-hidden="true" />
              <h3>Approve this Device?</h3>
            </header>
            <dl>
              <div>
                <dt>Name</dt>
                <dd>{{ approval.name }}</dd>
              </div>
              <div>
                <dt>Platform</dt>
                <dd>{{ approval.platform }}</dd>
              </div>
              <div>
                <dt>Device id</dt>
                <dd><code>{{ approval.device_id }}</code></dd>
              </div>
            </dl>
            <p class="panel-copy">
              A human on this Device must approve. Only then does this Device send the User id,
              shared profile metadata, and the secret key. The master password is never sent.
            </p>
          </div>

          <div class="vault-precheck">
            <h3>Vaults this Device will carry</h3>
            <p class="panel-copy">
              Pre-checked for you. Uncheck any Vault the new Device should not carry; you can edit
              its Sync Set later.
            </p>

            <ul v-if="vaults.length > 0">
              <li v-for="vault in vaults" :key="vault.vault_id">
                <label>
                  <input
                    type="checkbox"
                    :checked="checked.has(vault.vault_id)"
                    @change="toggleVault(vault.vault_id)"
                  />
                  {{ vaultLabel(vault) }}
                </label>
              </li>
            </ul>
            <p v-else class="muted">No Vaults are registered on this Device.</p>
          </div>

          <div class="panel-actions">
            <Button variant="label" :disabled="busy" @click="reset">Cancel</Button>
            <Button variant="accent" :disabled="busy" @click="approve">
              Approve and send access
            </Button>
          </div>
        </div>
      </template>

      <template v-else>
        <div class="transfer-ready">
          <ShieldCheck :size="24" aria-hidden="true" />
          <div>
            <h3>Access prepared for {{ approval?.name ?? 'the new Device' }}</h3>
            <p class="panel-copy">
              The encrypted transfer exists, but this build does not expose the secure pairing
              channel that delivers it. Nothing has been sent, and the new Device cannot finish
              pairing yet.
            </p>
          </div>
        </div>
        <Button variant="label" @click="emit('close')">Close</Button>
      </template>
    </section>

    <!-- Joining Device: code entry, then wait for source approval -->
    <section v-else class="pairing-panel">
      <template v-if="!joinMessage">
        <div class="code-mode">
          <label>
            <input v-model="codeMode" type="radio" value="typed" />
            Type the short code
          </label>
          <label>
            <input v-model="codeMode" type="radio" value="qr" />
            Paste the QR payload
          </label>
        </div>

        <template v-if="codeMode === 'typed'">
          <label class="field-label" for="pairing-source-id">Source Device id</label>
          <Input
            id="pairing-source-id"
            v-model="sourceDeviceId"
            type="text"
            autocomplete="off"
            placeholder="Shown on the other Device"
          />

          <label class="field-label" for="pairing-code">Pairing code</label>
          <Input
            id="pairing-code"
            v-model="typedCode"
            type="text"
            autocomplete="off"
            placeholder="ABCD-FGHJ"
          />
        </template>

        <template v-else>
          <label class="field-label" for="pairing-payload">QR payload</label>
          <textarea
            id="pairing-payload"
            v-model="qrPayload"
            class="frame-input thin-scrollbar"
            rows="4"
            spellcheck="false"
            placeholder="lockr:1:…"
          ></textarea>
        </template>

        <p class="panel-copy">
          The master password is typed on this Device only. It never crosses the network.
        </p>

        <div class="panel-actions">
          <Button variant="accent" :disabled="busy" @click="beginJoin">
            Continue
          </Button>
        </div>
      </template>

      <template v-else>
        <div class="approval-card">
          <header>
            <LoaderCircle :size="20" class="spin" aria-hidden="true" />
            <h3>Waiting for the source Device</h3>
          </header>

          <p v-if="joinInfo" class="panel-copy">
            This Device announced itself as <strong>{{ joinInfo.name }}</strong> on
            {{ joinInfo.platform }}. A human on the source Device must approve it by name and
            platform before any key material moves.
          </p>

          <p class="panel-copy">
            This build does not expose the secure pairing channel that carries the approval and
            key transfer, so pairing cannot complete from here yet. Nothing has been written.
          </p>

          <div class="panel-actions">
            <Button variant="label" :disabled="busy" @click="reset">Start over</Button>
            <Button variant="outline" @click="emit('close')">Close</Button>
          </div>
        </div>
      </template>
    </section>
  </article>
</template>

<style scoped>
.pairing-modal {
  display: flex;
  flex-direction: column;
  gap: 1.25rem;
}

.pairing-header {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
}

.pairing-title {
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

.pairing-subtitle,
.panel-copy {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
  line-height: 1.5;
}

.mode-tabs {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 0.375rem;
  padding: 0.25rem;
  border-radius: 0.625rem;
  background: var(--color-hover);

  button {
    padding: 0.5rem;
    border: none;
    border-radius: 0.375rem;
    background: transparent;
    color: var(--color-text-muted);
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
  }

  button.active {
    background: var(--color-bg);
    color: var(--color-text);
    box-shadow: var(--shadow-sm);
  }
}

.pairing-panel {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.offer-card {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;
  padding: 1rem;
  border: 1px solid var(--color-accent);
  border-radius: 0.625rem;
  background: var(--color-accent-hover);
}

.offer-top {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 1rem;
}

.offer-label {
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--color-accent);
}

.countdown {
  font-size: 0.75rem;
  color: var(--color-text-secondary);
}

.countdown.expired {
  color: var(--color-red);
}

.typed-code {
  font-family: ui-monospace, monospace;
  font-size: 1.75rem;
  font-weight: 600;
  letter-spacing: 0.12em;
  text-align: center;
  color: var(--color-text);
}

.payload-details {
  font-size: 0.75rem;
  color: var(--color-text-secondary);

  summary {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    cursor: pointer;
  }
}

.payload {
  display: block;
  margin: 0.5rem 0;
  padding: 0.5rem;
  max-height: 6rem;
  overflow: auto;
  overflow-wrap: anywhere;
  background: var(--color-bg);
  border-radius: 0.375rem;
  font-size: 0.6875rem;
}

.channel-step,
.approval-step,
.vault-precheck {
  display: flex;
  flex-direction: column;
  gap: 0.625rem;

  h3 {
    font-family: var(--font-geo);
    font-size: 0.9375rem;
    font-weight: 500;
  }
}

.frame-input {
  box-sizing: border-box;
  width: 100%;
  padding: 0.625rem;
  resize: vertical;
  background: var(--color-bg);
  color: var(--color-text);
  border: 1px solid var(--color-border);
  border-radius: 0.5rem;
  font-family: ui-monospace, monospace;
  font-size: 0.75rem;
}

.panel-actions {
  display: flex;
  justify-content: flex-end;
  gap: 0.5rem;
  flex-wrap: wrap;
}

.approval-card {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0.875rem;
  border: 1px solid var(--color-border);
  border-radius: 0.625rem;
  background: var(--color-bg-nav);

  header {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    color: var(--color-accent);
  }

  h3 {
    font-family: var(--font-geo);
    font-size: 1rem;
    font-weight: 500;
    color: var(--color-text);
  }

  dl {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
    gap: 0.625rem;
  }

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

.vault-precheck ul {
  display: flex;
  flex-direction: column;
  gap: 0.375rem;
  list-style: none;
}

.vault-precheck li label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  font-size: 0.8125rem;
  cursor: pointer;
}

.vault-precheck input {
  accent-color: var(--color-accent);
}

.transfer-ready {
  display: flex;
  align-items: flex-start;
  gap: 0.75rem;
  padding: 0.875rem;
  border-radius: 0.625rem;
  background: hsl(131 50% 96%);
  border: 1px solid hsl(131 40% 82%);
  color: hsl(131 40% 25%);

  h3 {
    font-family: var(--font-geo);
    font-size: 1rem;
    font-weight: 500;
    color: hsl(131 40% 22%);
    margin-bottom: 0.25rem;
  }
}

.code-mode {
  display: flex;
  gap: 1rem;

  label {
    display: flex;
    align-items: center;
    gap: 0.375rem;
    font-size: 0.8125rem;
    cursor: pointer;
  }

  input {
    accent-color: var(--color-accent);
  }
}

.field-label {
  margin-bottom: -0.375rem;
  color: var(--color-text-muted);
  font-size: 0.75rem;
}

.muted {
  color: var(--color-text-muted);
  font-size: 0.8125rem;
}

.error-text {
  color: var(--color-red);
  font-size: 0.8125rem;
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
</style>
