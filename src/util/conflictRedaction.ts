/**
 * Pure decision logic for the conflict prompt's secret reveal.
 *
 * The invariant this file exists to make testable: a secret's value is never
 * rendered unless the User asked for that exact side to be revealed. A
 * Conflict report never carries a secret value, but the display path must not
 * depend on that — it ignores the reported value for secret fields entirely.
 */

import type { ConflictChoice, SyncConflictSide } from '../types';

/** One side's identity, enough to key its reveal without the payload. */
export interface ConflictSideIdentity {
  device: string;
  counter: number;
}

/** One conflict side, ready for presentation. */
export interface ConflictSideEntry {
  choice: ConflictChoice | null;
  side: SyncConflictSide;
  label: string;
}

/** Revealed secret values, keyed by [`conflictSideKey`]. `null` means "not set". */
export type RevealedConflictValues = Readonly<Record<string, string | null>>;

/** The copy shown for a secret that has not been explicitly revealed. */
export const HIDDEN_SECRET = 'Value hidden';

/** Stable key for one conflict side: the writer identity. */
export function conflictSideKey(side: ConflictSideIdentity): string {
  return `${side.device}:${side.counter}`;
}

/** Whether this exact side has an explicit reveal recorded. */
export function isSideRevealed(
  revealed: RevealedConflictValues,
  side: ConflictSideIdentity
): boolean {
  return Object.prototype.hasOwnProperty.call(revealed, conflictSideKey(side));
}

/** Record one explicit reveal. The value may be null when the field is unset. */
export function revealSide(
  revealed: RevealedConflictValues,
  side: ConflictSideIdentity,
  value: string | null
): RevealedConflictValues {
  return { ...revealed, [conflictSideKey(side)]: value };
}

/** Drop one side's reveal, re-masking it. Unknown sides are a no-op. */
export function hideSide(
  revealed: RevealedConflictValues,
  side: ConflictSideIdentity
): RevealedConflictValues {
  const key = conflictSideKey(side);
  if (!Object.prototype.hasOwnProperty.call(revealed, key)) return revealed;

  const next = { ...revealed };
  delete next[key];
  return next;
}

/** Drop every reveal, as when moving to another Conflict. */
export function clearReveals(): RevealedConflictValues {
  return {};
}

/** Format a non-secret field value from the Conflict report. */
export function formatConflictValue(value: unknown): string {
  if (value === null || value === undefined) return 'Not set';

  if (typeof value === 'string') {
    return value.length > 0 ? value : 'Empty';
  }

  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  if (typeof value === 'number') return String(value);

  if (Array.isArray(value)) {
    return value.length > 0 ? value.join(', ') : 'None';
  }

  return JSON.stringify(value);
}

export interface ConflictSideDisplay {
  isSecret: boolean;
  /** Value from the Conflict report. Always null for a secret. */
  reportedValue: unknown;
  /** Whether the User explicitly revealed this exact side. */
  revealed: boolean;
  /** The explicitly fetched value; null means the field is not set. */
  revealedValue?: string | null;
}

/**
 * The value a side may render.
 *
 * For a secret field the reported value is ignored outright: without an
 * explicit reveal this returns [`HIDDEN_SECRET`], even if a caller mistakenly
 * passes the secret in `reportedValue`.
 */
export function displayConflictSideValue(input: ConflictSideDisplay): string {
  if (input.isSecret) {
    if (!input.revealed) return HIDDEN_SECRET;
    return input.revealedValue === null || input.revealedValue === undefined
      ? 'Not set'
      : input.revealedValue;
  }

  return formatConflictValue(input.reportedValue);
}
