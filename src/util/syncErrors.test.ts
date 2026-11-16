import { describe, expect, it } from 'vitest';
import { classifySyncError, type SyncFailureKind } from './syncErrors';

interface Expected {
  code: string;
  kind: SyncFailureKind;
  canRetry: boolean;
  canRecoverIdentity?: boolean;
}

const CODES: Expected[] = [
  { code: 'device.identity_damaged', kind: 'damaged-identity', canRetry: false, canRecoverIdentity: true },
  { code: 'sync.this_device_revoked', kind: 'this-device-revoked', canRetry: false },
  { code: 'sync.device_revoked', kind: 'revoked', canRetry: false },
  { code: 'sync.device_unreachable', kind: 'unreachable', canRetry: true },
  { code: 'sync.device_not_paired', kind: 'unpaired', canRetry: false },
  { code: 'sync.schema_mismatch', kind: 'schema-mismatch', canRetry: false },
  { code: 'pairing.schema_mismatch', kind: 'schema-mismatch', canRetry: false },
  { code: 'sync.cancelled', kind: 'cancelled', canRetry: true },
  { code: 'sync.interrupted', kind: 'interrupted', canRetry: true },
  { code: 'sync.timeout', kind: 'timeout', canRetry: true },
  { code: 'sync.discovery_failed', kind: 'discovery', canRetry: true },
  { code: 'sync.transport_unavailable', kind: 'transport', canRetry: false },
  { code: 'sync.authentication_failed', kind: 'authentication', canRetry: false },
  { code: 'sync.not_logged_in', kind: 'not-logged-in', canRetry: false },
  { code: 'sync.busy', kind: 'busy', canRetry: false }
];

describe('classifySyncError', () => {
  it.each(CODES)('maps $code to $kind', ({ code, kind, canRetry, canRecoverIdentity }) => {
    const failure = classifySyncError({ code, message: 'backend text' });

    expect(failure.kind).toBe(kind);
    expect(failure.canRetry).toBe(canRetry);
    expect(failure.canRecoverIdentity).toBe(canRecoverIdentity ?? false);
    expect(failure.detail).toBe('backend text');
    expect(failure.raw).toBe('backend text');
    expect(failure.title.length).toBeGreaterThan(0);
    expect(failure.advice.length).toBeGreaterThan(0);
  });

  it('classifies by the stable code, not by the message text', () => {
    const failure = classifySyncError({
      code: 'sync.device_revoked',
      message: 'Everything is fine, definitely try again'
    });

    expect(failure.kind).toBe('revoked');
    expect(failure.canRetry).toBe(false);
  });

  it('leaves an unrecognised code on the generic failure', () => {
    const failure = classifySyncError({ code: 'sync.future_code', message: 'who knows' });

    expect(failure.kind).toBe('unknown');
    expect(failure.canRetry).toBe(true);
    expect(failure.canRecoverIdentity).toBe(false);
    expect(failure.detail).toBe('who knows');
  });

  it('treats a failure with no code as unknown and keeps the display text', () => {
    const failure = classifySyncError('plain string failure');

    expect(failure.kind).toBe('unknown');
    expect(failure.detail).toBe('plain string failure');
  });

  it('serialises an object without a message rather than losing it', () => {
    const failure = classifySyncError({ code: 'sync.busy' });

    expect(failure.kind).toBe('busy');
    expect(failure.detail).toBe('{"code":"sync.busy"}');
  });
});
