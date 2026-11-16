import { markRaw, ref } from 'vue';
import AlertModal from '../components/ui/AlertModal.vue';
import PairingModal from '../components/devices/PairingModal.vue';
import { useModal } from './useModal';
import { useSync } from './useSync';
import type { PairedDevice } from '../types';
import { deviceLabel } from '../util/devices';

interface DeviceAdminOptions {
  refreshDevices: () => Promise<void>;
  /** Refresh the lists a pairing outcome can change. */
  onChanged: () => void | Promise<void>;
}

/**
 * Device actions that end in a confirmation dialog: admission, revocation and
 * identity recovery. The dialogs keep the exact user-facing copy that names
 * what travels to other Devices.
 */
export function useDeviceAdmin(options: DeviceAdminOptions) {
  const { error, revokeDevice, startWithNewDeviceIdentity } = useSync();
  const { openModal } = useModal();

  const actionError = ref<string | null>(null);

  function openPairing() {
    openModal(markRaw(PairingModal), {
      onClose: () => {
        void options.onChanged();
      }
    });
  }

  function confirmRevoke(device: PairedDevice) {
    const label = deviceLabel(device);
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
          await options.refreshDevices();
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

  return {
    actionError,
    openPairing,
    confirmRevoke,
    recoverIdentity
  };
}
