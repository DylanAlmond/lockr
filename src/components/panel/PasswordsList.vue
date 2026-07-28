<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { Account, AccountFilter } from '../../types';
import { useRoute, useRouter } from 'vue-router';
import { useVault } from '../../composables/useVault';
import { useSearch } from '../../composables/useSearch';
import Button from '../ui/Button.vue';
import { ArrowDownAZ, ArrowDownWideNarrow, ArrowUpWideNarrow, ArrowDownZA } from '@lucide/vue';
import useAppStore from '../../stores/appStore.ts';

type Props = AccountFilter & {
  recently_accessed: boolean;
};

type SortCategory = 'date' | 'alphabetical';

const props = withDefaults(defineProps<Props>(), {
  vault_id: null,
  favourite_only: false,
  recently_accessed: false,
  tags: null,
  search_query: null
});

const route = useRoute();
const router = useRouter();

const { searchQuery } = useSearch();
const { getAllAccounts } = useVault();

const accounts = ref<Account[]>([]);
const selectedTag = ref<string>('All');

const sortCategory = ref<SortCategory>('alphabetical');
const sortAsc = ref(sortCategory.value === 'alphabetical'); // false = newest first (descending)

const monthNames = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December'
];

// Build tag options from loaded accounts
// const tagOptions = computed(() => {
//   const tagSet = new Set<string>();
//   accounts.value.forEach((a) => a.tags.forEach((t) => tagSet.add(t)));
//   return ['All', ...Array.from(tagSet).sort()];
// });

// Filtered + sorted accounts
const filteredAccounts = computed(() => {
  let list =
    selectedTag.value === 'All'
      ? [...accounts.value]
      : accounts.value.filter((a) => a.tags.includes(selectedTag.value));

  list.sort((a, b) => {
    let comparison = 0;

    if (sortCategory.value === 'date') {
      comparison = new Date(a.updated_at).getTime() - new Date(b.updated_at).getTime();
    }

    if (sortCategory.value === 'alphabetical') {
      comparison = (a.display_name || a.username).localeCompare(b.display_name || b.username);
    }

    return sortAsc.value ? comparison : -comparison;
  });

  return list;
});

interface AccountGroup {
  label: string;
  items: Account[];
}

const groupedAccounts = computed<AccountGroup[]>(() => {
  const groups = new Map<string, Account[]>();

  for (const account of filteredAccounts.value) {
    let label: string;

    if (sortCategory.value === 'date') {
      const d = new Date(account.updated_at);
      label = `${monthNames[d.getMonth()]} ${d.getFullYear()}`;
    } else {
      label = (account.display_name || account.username)[0].toUpperCase();
    }

    if (!groups.has(label)) {
      groups.set(label, []);
    }

    groups.get(label)!.push(account);
  }

  return Array.from(groups.entries()).map(([label, items]) => ({
    label,
    items
  }));
});

// function selectTag(tag: string) {
//   selectedTag.value = tag;
// }

function setSortCategory(category: SortCategory) {
  sortCategory.value = category;
  sortAsc.value = category === 'alphabetical';
}

function toggleSort() {
  sortAsc.value = !sortAsc.value;
}

function accountRoute(accountId: string) {
  return {
    name: route.name,
    params: {
      ...route.params,
      passwordId: accountId
    }
  };
}

// Reload accounts whenever the filter or search changes
watch(
  () => [
    props.vault_id,
    props.favourite_only,
    props.tags,
    props.recently_accessed,
    searchQuery.value
  ],
  async () => {
    const nextAccounts = await getAllAccounts({
      ...props,
      search_query: searchQuery.value
    });

    accounts.value = nextAccounts;
  },
  { immediate: true }
);

// Only reset the default sort when the view changes
watch(
  () => [props.vault_id, props.favourite_only, props.recently_accessed],
  () => {
    sortCategory.value = props.recently_accessed ? 'date' : 'alphabetical';
    sortAsc.value = sortCategory.value === 'alphabetical';
  },
  { immediate: true }
);

// Default to first account in list
watch(
  [filteredAccounts, () => route.params.passwordId],
  ([accounts, currentId]) => {
    const id = currentId as string | undefined;

    if (!accounts.length) return;

    // Check if the currently routed ID exists in our filtered list
    const selectedExists = id ? accounts.some((account) => account.id === id) : false;

    // If no ID is present (or an invalid one is), default to the first account
    if (!selectedExists) {
      router.replace({
        name: route.name as string,
        params: {
          ...route.params,
          passwordId: accounts[0].id
        }
      });
    }
  },
  { immediate: true }
);
</script>

<template>
  <div class="container">
    <div class="filter-wrapper">
      <select
        :value="sortCategory"
        @change="setSortCategory(($event.target as HTMLSelectElement).value as SortCategory)"
      >
        <option value="date">Date Updated</option>
        <option value="alphabetical">Alphabetical</option>
      </select>

      <Button
        @click="toggleSort"
        icon-only
        variant="outline"
        :icon-component="
          sortCategory === 'date'
            ? sortAsc
              ? ArrowDownWideNarrow
              : ArrowUpWideNarrow
            : sortAsc
              ? ArrowDownAZ
              : ArrowDownZA
        "
        :aria-label="
          sortCategory === 'date'
            ? sortAsc
              ? 'Oldest First'
              : 'Newest First'
            : sortAsc
              ? 'A-Z'
              : 'Z-A'
        "
      >
      </Button>
    </div>

    <div class="results-wrapper no-scrollbar">
      <div v-if="groupedAccounts.length === 0">
        <p>No accounts found.</p>
      </div>

      <div v-for="group in groupedAccounts" class="account-group" :key="group.label">
        <h2>{{ group.label }}</h2>

        <ul class="account-list">
          <li v-for="account in group.items" :key="account.id">
            <RouterLink class="account" :to="accountRoute(account.id)" active-class="active">
              <div class="account-icon">
                {{ (account.display_name || account.username)[0].toUpperCase() }}
              </div>

              <div class="account-meta">
                <h3>{{ account.display_name || account.username }}</h3>
                <span>{{ account.email || account.username }}</span>
              </div>
            </RouterLink>
          </li>
        </ul>
      </div>
    </div>
  </div>
</template>

<style scoped>
.container {
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  padding-top: 1rem;
  padding-left: 0.5rem;
  padding-right: 0.5rem;

  height: 100%;
}

.filter-wrapper {
  display: flex;
  justify-content: space-between;
  padding-right: 0.5rem;
}

.results-wrapper {
  flex: 1;
  overflow-y: auto;
  padding-bottom: 1rem;
}

.account-group {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  padding-top: 3rem;

  > h2 {
    font-size: 1rem;
    font-family: var(--font-geo);
    color: var(--color-text-tertiary);
    font-weight: 400;
    padding-left: 0.5rem;
  }
}

.account-list {
  display: flex;
  flex-direction: column;
  list-style: none;
  gap: 0.5rem;
}

.account {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.5rem;

  text-decoration: none;

  cursor: pointer;
  user-select: none;

  font: inherit;
  font-size: 1rem;
  line-height: 1;
  color: inherit;

  border-radius: 0.75rem;

  transition: all 0.2s ease;

  &.active {
    box-shadow: var(--inset-sm);
    background-color: var(--color-hover);
  }

  &:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  &:hover {
    background-color: var(--color-hover);
  }

  /* Corner smoothing */
  @supports (corner-shape: squircle) {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}

.account-icon {
  display: flex;
  justify-content: center;
  align-items: center;
  width: 3.5rem;
  height: 3.5rem;
  aspect-ratio: 1/1;

  font-size: 1.25rem;
  font-family: var(--font-geo);
  font-weight: 500;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);

  border-radius: 0.375rem;
  box-shadow: var(--shadow-sm);
}

.account-meta {
  > h3 {
    font-size: 1rem;
    font-family: var(--font-geo);
    font-weight: 550;
    margin-bottom: 0.25rem;
  }

  > span {
    color: var(--color-text-tertiary);
    font-size: 0.875rem;
  }
}
</style>
