/**
 * Classification of backend sync failures by their stable error code.
 *
 * The code is the discriminator; the backend message is display text only and
 * may be reworded without changing behaviour. Kept free of Vue and Tauri so the
 * mapping is exercised directly by the frontend harness.
 */

/**
 * The distinct ways a sync can fail. Each one has its own title and advice so
 * the Devices screen never reduces "could not reach the laptop" and "that
 * Device was revoked" to the same red box.
 */
export type SyncFailureKind =
  | 'unreachable'
  | 'revoked'
  | 'unpaired'
  | 'this-device-revoked'
  | 'schema-mismatch'
  | 'damaged-identity'
  | 'cancelled'
  | 'interrupted'
  | 'timeout'
  | 'transport'
  | 'discovery'
  | 'authentication'
  | 'not-logged-in'
  | 'busy'
  | 'unknown';

export interface SyncFailure {
  kind: SyncFailureKind;
  title: string;
  detail: string;
  advice: string;
  /** True when retrying the same peer is a reasonable next step. */
  canRetry: boolean;
  /** Only a damaged Device identity offers this way out. */
  canRecoverIdentity: boolean;
  raw: string;
}

/**
 * A backend failure arrives as `{ code, message }`. The code is the stable
 * discriminator; the message is display text and may be reworded freely.
 */
function syncErrorInfo(error: unknown): { code: string | null; raw: string } {
  if (typeof error === 'object' && error !== null) {
    const candidate = error as { code?: unknown; message?: unknown };
    return {
      code: typeof candidate.code === 'string' ? candidate.code : null,
      raw:
        typeof candidate.message === 'string'
          ? candidate.message
          : JSON.stringify(candidate)
    };
  }

  return { code: null, raw: String(error) };
}

/** Classify a backend sync failure by its stable error code. */
export function classifySyncError(error: unknown): SyncFailure {
  const { code, raw } = syncErrorInfo(error);

  const failure = (
    kind: SyncFailureKind,
    title: string,
    advice: string,
    canRetry = true,
    canRecoverIdentity = false
  ): SyncFailure => ({
    kind,
    title,
    detail: raw,
    advice,
    canRetry,
    canRecoverIdentity,
    raw
  });

  switch (code) {
    case 'device.identity_damaged':
      return failure(
        'damaged-identity',
        'This Device’s identity is damaged',
        'Lockr will not replace this Device’s identity on its own. Restore the identity file from a backup, or start again with a new identity below.',
        false,
        true
      );

    case 'sync.this_device_revoked':
      return failure(
        'this-device-revoked',
        'This Device has been revoked',
        'No honest Device will sync with this copy any more. Pair this installation again from another Device to rejoin with a new identity.',
        false
      );

    case 'sync.device_revoked':
      return failure(
        'revoked',
        'That Device has been revoked',
        'Revocation is permanent. This Device will not sync with it again, even if its old copy of the Device list disagrees.',
        false
      );

    case 'sync.device_unreachable':
      return failure(
        'unreachable',
        'That Device could not be reached',
        'Check that the other Device is running Lockr on the same network, then try again. Nothing was changed.',
        true
      );

    case 'sync.device_not_paired':
      return failure(
        'unpaired',
        'That Device is not paired with this one',
        'Pairing is the only grant of access. Admit the Device from the Devices screen before syncing with it.',
        false
      );

    case 'sync.schema_mismatch':
    case 'pairing.schema_mismatch':
      return failure(
        'schema-mismatch',
        'The two Devices use different vault schemas',
        'Update the older Device. Lockr will not send data the other Device cannot read.',
        false
      );

    case 'sync.cancelled':
      return failure(
        'cancelled',
        'Sync cancelled',
        'Any Vault that finished transferring is safe to open. Everything else was left as it was before the sync.',
        true
      );

    case 'sync.interrupted':
      return failure(
        'interrupted',
        'The connection dropped mid-sync',
        'Every Vault is still readable and the sync is safe to retry. A complete Vault is the smallest unit transferred.',
        true
      );

    case 'sync.timeout':
      return failure(
        'timeout',
        'The other Device stopped responding',
        'Try again once it is awake and on the network.',
        true
      );

    case 'sync.discovery_failed':
      return failure(
        'discovery',
        'Devices could not be discovered',
        'Discovery needs the local network. Check the connection and try refreshing the list.',
        true
      );

    case 'sync.transport_unavailable':
      return failure(
        'transport',
        'The secure sync channel is unavailable',
        'This build cannot open a channel to another Device. No vault data was sent.',
        false
      );

    case 'sync.authentication_failed':
      return failure(
        'authentication',
        'The other Device could not be authenticated',
        'The secure channel did not prove which Device answered. Re-pairing is the fallback.',
        false
      );

    case 'sync.not_logged_in':
      return failure(
        'not-logged-in',
        'Sign in to sync',
        'Sync uses the credentials already resident in this session. Sign in and try again.',
        false
      );

    case 'sync.busy':
      return failure(
        'busy',
        'A sync is already running',
        'Only one sync runs at a time on this Device. Wait for it to finish, or cancel it first.',
        false
      );

    default:
      return failure(
        'unknown',
        'The sync did not complete',
        'Nothing was reported as complete. Try again, or check the other Device.',
        true
      );
  }
}
