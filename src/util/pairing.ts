import type { User } from '../types';

/**
 * Whether this Device may start a join. The backend refuses a join whenever a
 * local User exists, so the UI decides that before showing the form instead of
 * reporting the refusal as an error after the fact.
 */
export interface JoinGate {
  canJoin: boolean;
  /** Why joining is refused, ready to show. `null` when it may join. */
  blockedReason: string | null;
}

export function joinGate(localUsers: readonly Pick<User, 'name'>[]): JoinGate {
  const existing = localUsers[0];

  if (!existing) {
    return { canJoin: true, blockedReason: null };
  }

  return {
    canJoin: false,
    blockedReason:
      `This Device already belongs to ${existing.name}. ` +
      'A Device that already has a User cannot join another User.'
  };
}

/**
 * One block with both values the typed join path asks for. The Device id is
 * otherwise only inside the QR payload, so the source shows this to anyone
 * without a camera.
 */
export function pairingHandoffText(offer: {
  typed_code: string;
  source_device_id: string;
}): string {
  return `Pairing code: ${offer.typed_code}\nDevice id: ${offer.source_device_id}`;
}

/** The typed path needs a source Device id and a code, both non-blank. */
export function typedJoinComplete(sourceDeviceId: string, code: string): boolean {
  return sourceDeviceId.trim().length > 0 && code.trim().length > 0;
}
