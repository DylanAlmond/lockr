import { ref } from 'vue';

const searchQuery = ref<string | undefined>(undefined);

export function useSearch() {
  function setSearch(query: string | undefined) {
    searchQuery.value = query;
  }

  function clearSearch() {
    searchQuery.value = undefined;
  }

  return {
    searchQuery,
    setSearch,
    clearSearch
  };
}
