import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { DuplicateOutcome, DuplicateResolution, DuplicateSuggestion } from '../types';

export function useDuplicates() {
  const suggestions = ref<DuplicateSuggestion[]>([]);
  const isLoading = ref(false);
  const error = ref<string | null>(null);

  async function listSuggestions(vaultId: string): Promise<DuplicateSuggestion[]> {
    isLoading.value = true;
    error.value = null;

    try {
      suggestions.value = await invoke<DuplicateSuggestion[]>('get_duplicate_suggestions', {
        vaultId
      });
      return suggestions.value;
    } catch (e) {
      error.value = String(e);
      suggestions.value = [];
      return [];
    } finally {
      isLoading.value = false;
    }
  }

  async function resolve(
    vaultId: string,
    suggestion: DuplicateSuggestion,
    resolution: DuplicateResolution
  ): Promise<DuplicateOutcome | null> {
    error.value = null;

    try {
      const outcome = await invoke<DuplicateOutcome>('resolve_duplicate', {
        vaultId,
        first: suggestion.first.item_id,
        second: suggestion.second.item_id,
        resolution
      });

      suggestions.value = suggestions.value.filter(
        (candidate) =>
          !(
            candidate.first.item_id === suggestion.first.item_id &&
            candidate.second.item_id === suggestion.second.item_id
          )
      );

      return outcome;
    } catch (e) {
      error.value = String(e);
      return null;
    }
  }

  return {
    suggestions,
    isLoading,
    error,
    listSuggestions,
    resolve
  };
}
