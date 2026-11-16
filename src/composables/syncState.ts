import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { SyncFailure } from '../util/syncErrors';
import type { ConflictChoice, SyncConflict, SyncProgress, SyncReport } from '../types';

/** One open sync session whose Conflicts hold it open. */
export interface ConflictSession {
  sessionId: string;
  peerDeviceId: string;
  conflicts: SyncConflict[];
}

export interface ConflictResolutionRequest {
  sessionId: string;
  vaultId: string;
  itemId: string | null;
  fieldKey: string;
  choice: ConflictChoice;
}

/** One on-demand secret reveal. `value: null` means the candidate is not set. */
export interface RevealedConflictValue {
  value: string | null;
}

// Sync progress is process-wide: a sync can outlive the screen that started it.
export const isSyncing = ref(false);
export const syncingPeerId = ref<string | null>(null);
export const progress = ref<SyncProgress[]>([]);
export const syncFailure = ref<SyncFailure | null>(null);
export const conflicts = ref<ConflictSession | null>(null);
export const moveResidueNotes = ref<SyncReport['duplicate_notes']>([]);
export const lastReport = ref<SyncReport | null>(null);
export const conflictError = ref<string | null>(null);
export const error = ref<string | null>(null);

export function clearSyncFailure(): void {
  syncFailure.value = null;
}

export function resetSyncState(): void {
  isSyncing.value = false;
  syncingPeerId.value = null;
  progress.value = [];
  syncFailure.value = null;
  conflicts.value = null;
  moveResidueNotes.value = [];
  lastReport.value = null;
  conflictError.value = null;
}

/**
 * One command call, one error convention: a failed call records the message
 * and yields null; a successful `void` command yields undefined, which is
 * still a success. Callers that return a boolean wrap this in `requestOk`.
 */
export async function request<T>(
  command: string,
  args?: Record<string, unknown>,
  clearError = false
): Promise<T | null> {
  if (clearError) error.value = null;

  try {
    return await invoke<T>(command, args);
  } catch (e) {
    error.value = String(e);
    return null;
  }
}

export async function requestOk(
  command: string,
  args?: Record<string, unknown>,
  clearError = false
): Promise<boolean> {
  if (clearError) error.value = null;

  try {
    // A `Result<(), _>` command resolves to `null`, which is a success.
    // This is deliberately separate from `request`, whose `null` means a
    // failed call for commands that return a value.
    await invoke(command, args);
    return true;
  } catch (e) {
    error.value = String(e);
    return false;
  }
}
