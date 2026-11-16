<script setup lang="ts">
import { Check, Copy, LoaderCircle, QrCode, ShieldCheck } from '@lucide/vue';
import Button from '../ui/Button.vue';
import { usePairingOffer } from '../../composables/usePairingOffer';
import { vaultLabel } from '../../util/labels';

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const {
  busy,
  localError,
  offer,
  vaults,
  checked,
  waitingForJoin,
  approval,
  outcome,
  copied,
  handoffCopied,
  secondsLeft,
  offerExpired,
  toggleVault,
  beginOffer,
  copyPayload,
  copyHandoff,
  approve,
  reset
} = usePairingOffer();
</script>

<template>
  <section class="pairing-panel">
    <p v-if="localError" class="error-text">{{ localError }}</p>

    <template v-if="!offer && !outcome">
      <p class="panel-copy">
        Start a single-use offer here, then read or scan it on the new Device. The new Device
        must have no User yet. The code expires after about a minute and can be used once.
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

        <div class="handoff">
          <div class="handoff-field">
            <span class="offer-label">Pairing code</span>
            <p class="typed-code">{{ offer.typed_code }}</p>
          </div>

          <div class="handoff-field">
            <span class="offer-label">This Device id</span>
            <code class="device-id">{{ offer.source_device_id }}</code>
          </div>

          <Button
            variant="outline"
            size="xs"
            :icon-component="handoffCopied ? Check : Copy"
            @click="copyHandoff"
          >
            {{ handoffCopied ? 'Copied code and Device id' : 'Copy code and Device id' }}
          </Button>
        </div>

        <p class="panel-copy">
          On the new Device, choose <strong>Join a Device</strong> and type both values. No
          camera is needed; the QR payload below is only a shortcut.
        </p>

        <details class="payload-details">
          <summary>
            <QrCode :size="15" aria-hidden="true" />
            QR payload
          </summary>
          <code class="payload">{{ offer.qr_payload }}</code>
          <Button
            variant="outline"
            size="xs"
            :icon-component="copied ? Check : Copy"
            @click="copyPayload"
          >
            {{ copied ? 'Copied' : 'Copy payload' }}
          </Button>
        </details>
      </div>

      <div v-if="waitingForJoin && !approval" class="channel-step">
        <h3>Waiting for the new Device</h3>
        <p class="panel-copy">
          <LoaderCircle :size="15" class="spin" aria-hidden="true" />
          This Device is listening over the secure pairing channel. The announcement appears here
          as soon as the new Device connects.
        </p>
        <div class="panel-actions">
          <Button variant="label" :disabled="busy" @click="reset">Cancel pairing</Button>
        </div>
      </div>

      <div v-else-if="!approval" class="channel-step">
        <h3>Pairing code expired</h3>
        <p class="panel-copy">Nothing was exchanged. Start a new code to try again.</p>
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
            holds the User id, shared profile metadata, and the secret key. It fetches the checked
            Vaults over the sync channel; the master password was never sent.
          </p>
        </div>
      </div>
      <Button variant="label" @click="emit('close')">Close</Button>
    </template>
  </section>
</template>

<style scoped src="./pairing-panel.css"></style>
