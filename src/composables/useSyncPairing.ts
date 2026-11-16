import { request, requestOk } from './syncState';
import type {
  PairingApprovalPrompt,
  PairingCode,
  PairingJoinReady,
  PairingOffer,
  PairingOutcome,
  User
} from '../types';

/** Pairing and inbound serving calls. The transfer never reaches the UI. */
export function useSyncPairing() {
  async function startPairing(): Promise<PairingOffer | null> {
    return request<PairingOffer>('start_pairing', undefined, true);
  }

  async function cancelPairing(): Promise<boolean> {
    return requestOk('cancel_pairing');
  }

  async function joinPairing(code: PairingCode): Promise<PairingJoinReady | null> {
    return request<PairingJoinReady>('join_pairing', { code }, true);
  }

  async function receivePairingRequest(): Promise<PairingApprovalPrompt | null> {
    return request<PairingApprovalPrompt>('receive_pairing_request', undefined, true);
  }

  async function approvePairing(
    deviceId: string,
    syncSet: string[] | null
  ): Promise<PairingOutcome | null> {
    return request<PairingOutcome>('approve_pairing', { deviceId, syncSet }, true);
  }

  /**
   * Joining Device: re-wrap the transferred secret key under the password
   * typed on this Device. The transfer never reaches the frontend.
   */
  async function completePairing(masterPassword: string): Promise<User | null> {
    return request<User>('complete_pairing', { masterPassword }, true);
  }

  /** Serve inbound sync. Advertising remains owned by the discovery session. */
  async function startServing(): Promise<boolean> {
    return requestOk('start_serving', undefined, true);
  }

  async function stopServing(): Promise<boolean> {
    return requestOk('stop_serving');
  }

  return {
    startPairing,
    cancelPairing,
    joinPairing,
    receivePairingRequest,
    approvePairing,
    completePairing,
    startServing,
    stopServing
  };
}
