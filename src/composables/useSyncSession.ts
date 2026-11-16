import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { classifySyncError } from '../util/syncErrors';
import {
  conflictError,
  conflicts,
  moveResidueNotes,
  error,
  isSyncing,
  lastReport,
  progress,
  requestOk,
  syncFailure,
  syncingPeerId,
  type ConflictResolutionRequest,
  type ConflictSession,
  type RevealedConflictValue
} from './syncState';
import type { ConflictResolutionResult, SyncConflict, SyncProgress, SyncReport } from '../types';

/** One sync session, its Conflicts, and their on-demand secret reveals. */
export function useSyncSession() {
  const isResolvingConflict = ref(false);

  function conflictsOf(session: ConflictSession, conflict: SyncConflict): SyncConflict[] {
    return session.conflicts.filter(
      (candidate) =>
        !(
          candidate.vault_id === conflict.vault_id &&
          candidate.field_key === conflict.field_key &&
          (candidate.item_id ?? null) === (conflict.item_id ?? null)
        )
    );
  }

  /**
   * Sync with one paired Device. Progress arrives on the `sync-progress`
   * event; a returned report with Conflicts keeps this session open until
   * each one is resolved or the User abandons the prompt.
   */
  async function syncWithDevice(deviceId: string): Promise<SyncReport | null> {
    if (isSyncing.value) {
      error.value = 'A sync is already running';
      return null;
    }

    isSyncing.value = true;
    syncingPeerId.value = deviceId;
    progress.value = [];
    syncFailure.value = null;
    conflicts.value = null;
    moveResidueNotes.value = [];
    lastReport.value = null;
    conflictError.value = null;

    try {
      const report = await invoke<SyncReport>('sync_with_device', { deviceId });
      lastReport.value = report;
      moveResidueNotes.value = report.duplicate_notes;
      if (report.conflicts.length > 0) {
        conflicts.value = {
          sessionId: report.session_id,
          peerDeviceId: report.peer_device_id,
          conflicts: report.conflicts
        };
      }
      return report;
    } catch (e) {
      syncFailure.value = classifySyncError(e);
      return null;
    } finally {
      isSyncing.value = false;
      syncingPeerId.value = null;
    }
  }

  async function cancelSync(): Promise<boolean> {
    return requestOk('cancel_sync');
  }

  function onSyncProgress(handler: (payload: SyncProgress) => void): Promise<UnlistenFn> {
    return listen<SyncProgress>('sync-progress', (event) => handler(event.payload));
  }

  /** Choose one side of a field Conflict. The backend writes only on success. */
  async function resolveSyncConflict(
    request: ConflictResolutionRequest
  ): Promise<ConflictResolutionResult | null> {
    isResolvingConflict.value = true;
    conflictError.value = null;

    try {
      const result = await invoke<ConflictResolutionResult>('resolve_sync_conflict', {
        sessionId: request.sessionId,
        vaultId: request.vaultId,
        itemId: request.itemId,
        fieldKey: request.fieldKey,
        choice: request.choice
      });

      const session = conflicts.value;
      if (session) {
        const conflict = session.conflicts.find(
          (candidate) =>
            candidate.vault_id === request.vaultId &&
            candidate.field_key === request.fieldKey &&
            (candidate.item_id ?? null) === (request.itemId ?? null)
        );
        const remaining = conflict ? conflictsOf(session, conflict) : session.conflicts;

        if (result.remaining === 0 || remaining.length === 0) {
          conflicts.value = null;
        } else {
          conflicts.value = { ...session, conflicts: remaining };
        }
      }

      return result;
    } catch (e) {
      conflictError.value = String(e);
      return null;
    } finally {
      isResolvingConflict.value = false;
    }
  }

  /**
   * Reveal one side of a secret Conflict. This is the per-field on-demand
   * fetch: the Conflict payload never carries either value, so a reveal must
   * be asked for explicitly and applies to one field at a time.
   */
  async function revealConflictCandidate(
    request: ConflictResolutionRequest
  ): Promise<RevealedConflictValue | null> {
    conflictError.value = null;

    try {
      const value = await invoke<string | null>('get_conflict_candidate_secret', {
        sessionId: request.sessionId,
        vaultId: request.vaultId,
        itemId: request.itemId,
        fieldKey: request.fieldKey,
        choice: request.choice
      });
      return { value };
    } catch (e) {
      conflictError.value = String(e);
      return null;
    }
  }

  /**
   * Dismiss the prompt without choosing. Clean merges are already saved.
   * The backend has no abandon command, so the session stays open until
   * logout; this only stops the UI blocking navigation.
   */
  function abandonConflicts(): void {
    conflicts.value = null;
    conflictError.value = null;
  }

  return {
    isResolvingConflict,
    syncWithDevice,
    cancelSync,
    onSyncProgress,
    resolveSyncConflict,
    revealConflictCandidate,
    abandonConflicts
  };
}
