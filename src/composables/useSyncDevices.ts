import { request, requestOk } from './syncState';
import type { DiscoveredPeer, PairedDevice } from '../types';

/** Paired Devices, discovery, and this installation's Device identity. */
export function useSyncDevices() {
  async function listPairedDevices(): Promise<PairedDevice[] | null> {
    return request<PairedDevice[]>('list_paired_devices');
  }

  async function startDiscovery(): Promise<boolean> {
    return requestOk('start_discovery');
  }

  async function stopDiscovery(): Promise<boolean> {
    return requestOk('stop_discovery');
  }

  async function listDiscoveredPeers(): Promise<DiscoveredPeer[] | null> {
    return request<DiscoveredPeer[]>('list_discovered_peers');
  }

  async function revokeDevice(deviceId: string): Promise<boolean> {
    return requestOk('revoke_device', { deviceId }, true);
  }

  async function getDeviceIdentityError(): Promise<string | null> {
    return request<string | null>('get_device_identity_error');
  }

  async function startWithNewDeviceIdentity(confirm: boolean): Promise<boolean> {
    return requestOk('start_with_new_device_identity', { confirm }, true);
  }

  return {
    listPairedDevices,
    startDiscovery,
    stopDiscovery,
    listDiscoveredPeers,
    revokeDevice,
    getDeviceIdentityError,
    startWithNewDeviceIdentity
  };
}
