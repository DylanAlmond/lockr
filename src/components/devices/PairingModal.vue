<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { Check, Copy, KeyRound, LoaderCircle, QrCode, ShieldCheck } from '@lucide/vue';
import Button from '../ui/Button.vue';
import Input from '../ui/Input.vue';
import { useSync } from '../../composables/useSync';
import type {
  PairingApprovalPrompt,
  PairingJoinReady,
  PairingOffer,
  PairingOutcome,
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
  completePairing,
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
const waitingForJoin = ref(false);
const approval = ref<PairingApprovalPrompt | null>(null);
const outcome = ref<PairingOutcome | null>(null);
const copied = ref(false);

// Joining Device
const codeMode = ref<'typed' | 'qr'>('typed');
const sourceDeviceId = ref('');
const typedCode = ref('');
const qrPayload = ref('');
const joinStage = ref<'idle' | 'waiting' | 'approved' | 'done'>('idle');
const joinReady = ref<PairingJoinReady | null>(null);
const joinPassword = ref('');
const joinAborted = ref(false);

const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;
let disposed = false;

onMounted(() => {
  timer = setInterval(() => {
    now.value = Date.now();
  }, 1000);
});

onUnmounted(() => {
  disposed = true;
  if (timer) clearInterval(timer);
  if (offer.value || waitingForJoin.value || joinStage.value === 'waiting' || joinStage.value === 'approved') {
    void cancelPairing();
  }
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

  if (offer.value || waitingForJoin.value || joinStage.value !== 'idle') {
    await cancelPairing();
  }

  mode.value = next;
  localError.value = null;
  offer.value = null;
  approval.value = null;
  outcome.value = null;
  joinStage.value = 'idle';
  joinReady.value = null;
  joinPassword.value = '';
  waitingForJoin.value = false;
  joinAborted.value = false;
  syncSetLoaded.value = false;
}

async function beginOffer() {
  busy.value = true;
  localError.value = null;
  outcome.value = null;
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

  // The backend owns the transport. This command blocks until a joining
  // Device actually announces itself; the UI never sees a handshake frame.
  void waitForApproval();
}

async function waitForApproval() {
  waitingForJoin.value = true;
  localError.value = null;

  const prompt = await receivePairingRequest();
  waitingForJoin.value = false;

  if (disposed || !offer.value) return;

  if (!prompt) {
    localError.value = offerExpired.value
      ? 'The pairing code expired. Start again.'
      : error.value ?? 'The joining Device could not be approved.';
    return;
  }

  approval.value = prompt;
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

async function approve() {
  if (!approval.value) return;

  busy.value = true;
  localError.value = null;
  const result = await approvePairing(
    approval.value.device_id,
    syncSetLoaded.value ? [...checked.value] : null
  );
  busy.value = false;

  if (!result) {
    localError.value = error.value;
    return;
  }

  outcome.value = result;
  approval.value = null;
  offer.value = null;
}

async function beginJoin() {
  busy.value = true;
  localError.value = null;
  joinAborted.value = false;
  joinStage.value = 'waiting';
  joinReady.value = null;

  const code =
    codeMode.value === 'qr'
      ? ({ kind: 'qr', payload: qrPayload.value.trim() } as const)
      : ({
          kind: 'typed',
          source_device_id: sourceDeviceId.value.trim(),
          code: typedCode.value.trim()
        } as const);

  const ready = await joinPairing(code);
  busy.value = false;

  if (joinAborted.value || disposed) return;

  if (!ready) {
    joinStage.value = 'idle';
    localError.value = error.value;
    return;
  }

  joinReady.value = ready;
  joinStage.value = 'approved';
}

async function finishJoin() {
  busy.value = true;
  localError.value = null;

  const user = await completePairing(joinPassword.value);
  busy.value = false;

  if (!user) {
    localError.value = error.value;
    return;
  }

  joinPassword.value = '';
  joinStage.value = 'done';
}

async function reset() {
  joinAborted.value = true;
  busy.value = true;
  await cancelPairing();
  busy.value = false;
  offer.value = null;
  approval.value = null;
  outcome.value = null;
  waitingForJoin.value = false;
  joinStage.value = 'idle';
  joinReady.value = null;
  joinPassword.value = '';
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
      <template v-if="!offer && !outcome">
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

        <div v-if="waitingForJoin && !approval" class="channel-step">
          <h3>Waiting for the new Device</h3>
          <p class="panel-copy">
            <LoaderCircle :size="15" class="spin" aria-hidden="true" />
            This Device is listening over the secure pairing channel. The announcement appears
            here as soon as the new Device connects.
          </p>
          <div class="panel-actions">
            <Button variant="label" :disabled="busy" @click="reset">Cancel pairing</Button>
          </div>
        </div>

        <div v-else-if="!approval" class="channel-step">
          <h3>Pairing code expired</h3>
          <p class="panel-copy">
            Nothing was exchanged. Start a new code to try again.
          </p>
          <div class="panel-actions">
            <Button variant="label" :disabled="busy" @click="reset">Start over</Button>
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
            <h3>Paired with {{ outcome?.name ?? 'the new Device' }}</h3>
            <p class="panel-copy">
              The approval was delivered over the secure pairing channel and the new Device now
              holds the User id, shared profile metadata, and the secret key. The master password
              was never sent.
            </p>
          </div>
        </div>
        <Button variant="label" @click="emit('close')">Close</Button>
      </template>
    </section>

    <!-- Joining Device: code entry, wait for source approval, then password -->
    <section v-else class="pairing-panel">
      <template v-if="joinStage === 'idle'">
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
            placeholder="lockr-pair:v1:…"
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

      <template v-else-if="joinStage === 'waiting'">
        <div class="approval-card">
          <header>
            <LoaderCircle :size="20" class="spin" aria-hidden="true" />
            <h3>Waiting for the source Device</h3>
          </header>

          <p class="panel-copy">
            This Device announced itself over the secure pairing channel. A human on the source
            Device must approve it by name and platform before any key material moves.
          </p>

          <div class="panel-actions">
            <Button variant="label" @click="reset">Cancel</Button>
          </div>
        </div>
      </template>

      <template v-else-if="joinStage === 'approved'">
        <div class="approval-card">
          <header>
            <ShieldCheck :size="20" aria-hidden="true" />
            <h3>Approved by the source Device</h3>
          </header>

          <p class="panel-copy">
            <strong>{{ joinReady?.user_name || 'Your account' }}</strong> approved this Device.
            Enter the master password for that account to finish pairing. It is used only on this
            Device.
          </p>

          <label class="field-label" for="pairing-password">Master password</label>
          <Input
            id="pairing-password"
            v-model="joinPassword"
            type="password"
            autocomplete="current-password"
            placeholder="Master password"
          />

          <div class="panel-actions">
            <Button variant="label" :disabled="busy" @click="reset">Cancel</Button>
            <Button variant="accent" :disabled="busy || !joinPassword" @click="finishJoin">
              Finish pairing
            </Button>
          </div>
        </div>
      </template>

      <template v-else>
        <div class="transfer-ready">
          <ShieldCheck :size="24" aria-hidden="true" />
          <div>
            <h3>This Device is paired</h3>
            <p class="panel-copy">
              The secret key was re-wrapped under the password typed here. Open the Devices screen
              to sync, or close this dialog.
            </p>
          </div>
        </div>
        <Button variant="label" @click="emit('close')">Close</Button>
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
