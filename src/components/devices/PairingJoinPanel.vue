<script setup lang="ts">
import { LoaderCircle, ShieldCheck } from '@lucide/vue';
import Button from '../ui/Button.vue';
import Input from '../ui/Input.vue';
import { usePairingJoin } from '../../composables/usePairingJoin';

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const {
  busy,
  localError,
  codeMode,
  sourceDeviceId,
  typedCode,
  qrPayload,
  joinStage,
  joinReady,
  joinPassword,
  joinSyncState,
  joinSyncMessage,
  beginJoin,
  finishJoin,
  runInitialSync,
  reset
} = usePairingJoin();
</script>

<template>
  <section class="pairing-panel">
    <p v-if="localError" class="error-text">{{ localError }}</p>

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
        <Button variant="accent" :disabled="busy" @click="beginJoin">Continue</Button>
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
          <strong>{{ joinReady?.user_name || 'Your User' }}</strong> approved this Device. Enter
          the master password for that User to finish pairing. It is used only on this Device.
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
        <LoaderCircle
          v-if="joinSyncState === 'syncing'"
          :size="24"
          class="spin"
          aria-hidden="true"
        />
        <ShieldCheck v-else :size="24" aria-hidden="true" />
        <div>
          <h3>
            {{ joinSyncState === 'syncing' ? 'Fetching your Vaults' : 'This Device is paired' }}
          </h3>
          <p class="panel-copy">
            <template v-if="joinSyncState === 'syncing'">
              Pairing succeeded. Your Vaults are arriving over the secure sync channel; this can
              take a moment.
            </template>
            <template v-else-if="joinSyncState === 'failed'">
              Pairing succeeded, but the first Vault sync did not finish. Nothing was lost; you
              can retry, or sync later from the Devices screen.
            </template>
            <template v-else>
              {{ joinSyncMessage }}
            </template>
          </p>
          <p v-if="joinSyncState === 'failed'" class="error-text">{{ joinSyncMessage }}</p>
        </div>
      </div>
      <div class="panel-actions">
        <Button
          v-if="joinSyncState === 'failed'"
          variant="outline"
          :disabled="busy"
          @click="runInitialSync"
        >
          Retry Vault sync
        </Button>
        <Button
          variant="label"
          :disabled="busy || joinSyncState === 'syncing'"
          @click="emit('close')"
        >
          Close
        </Button>
      </div>
    </template>
  </section>
</template>

<style scoped src="./pairing-panel.css"></style>
