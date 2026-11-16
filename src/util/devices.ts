import type { DiscoveredPeer, PairedDevice, SyncSetView } from '../types';
import { shortId } from './labels';
import { formatTimestamp } from './timestamp';

/** Vault id → display name, with a short-id fallback for unnamed Vaults. */
export function vaultNamesFrom(set: SyncSetView | null): Record<string, string> {
  const names: Record<string, string> = {};
  for (const vault of set?.vaults ?? []) {
    names[vault.vault_id] = vault.name?.trim() || `Vault ${vault.vault_id.slice(0, 8)}`;
  }
  return names;
}

/** Device id → display name. */
export function deviceNamesFrom(devices: PairedDevice[]): Record<string, string> {
  const names: Record<string, string> = {};
  for (const device of devices) {
    names[device.device_id] = device.name;
  }
  return names;
}

/** The row for this installation, if the Device list has been read. */
export function thisDeviceOf(devices: PairedDevice[]): PairedDevice | null {
  return devices.find((device) => device.is_this_device) ?? null;
}

/** Peers excluding this installation: seeing yourself is not a peer. */
export function visiblePeersOf(
  peers: DiscoveredPeer[],
  thisDevice: PairedDevice | null
): DiscoveredPeer[] {
  return peers.filter((peer) => peer.device_id !== thisDevice?.device_id);
}

/** Every paired Device except this installation. */
export function pairedPeersOf(devices: PairedDevice[]): PairedDevice[] {
  return devices.filter((device) => !device.is_this_device);
}

/** The most recent `last_sync_at` across all Devices, as an ISO string. */
export function latestLastSyncOf(devices: PairedDevice[]): string | null {
  const times = devices
    .map((device) => device.last_sync_at)
    .filter((value): value is string => !!value)
    .map((value) => new Date(value).getTime())
    .filter((value) => Number.isFinite(value));

  if (times.length === 0) return null;
  return new Date(Math.max(...times)).toISOString();
}

export function deviceForPeer(
  devices: PairedDevice[],
  peer: DiscoveredPeer
): PairedDevice | undefined {
  return devices.find((device) => device.device_id === peer.device_id);
}

/** A discovered peer's name from the Device list, or an opaque fallback. */
export function peerLabel(devices: PairedDevice[], peer: DiscoveredPeer): string {
  return deviceForPeer(devices, peer)?.name ?? `Unknown Device ${shortId(peer.device_id)}`;
}

export type PeerStatus = 'this' | 'unknown' | 'revoked' | 'paired';

/**
 * This is the only place a discovered peer is promoted to "paired". Discovery
 * alone never grants access; the Device list is the source of truth.
 */
export function peerStatus(devices: PairedDevice[], peer: DiscoveredPeer): PeerStatus {
  const device = deviceForPeer(devices, peer);
  if (device?.is_this_device) return 'this';
  if (!peer.paired) return 'unknown';
  if (device?.revoked) return 'revoked';
  return 'paired';
}

export function lastSyncLabel(device: PairedDevice): string {
  if (device.is_this_device) return 'This Device';
  return device.last_sync_at
    ? `Last synced ${formatTimestamp(device.last_sync_at)}`
    : 'Never synced with this Device';
}

/** A Device's name with the short-id fallback used in dialogs and rows. */
export function deviceLabel(device: PairedDevice): string {
  return device.name.trim() || `Device ${shortId(device.device_id)}`;
}

/** Desktop platforms get the laptop glyph; everything else is a phone. */
export function isDesktopPlatform(platform: string): boolean {
  const normalised = platform.toLowerCase();
  return (
    normalised.includes('mac') ||
    normalised.includes('linux') ||
    normalised.includes('windows')
  );
}
