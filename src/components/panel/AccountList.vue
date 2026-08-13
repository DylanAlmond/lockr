<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { Account, AccountFilter } from '../../types/index.ts';
import { useRoute, useRouter } from 'vue-router';
import { useVault } from '../../composables/useVault.ts';
import { useSearch } from '../../composables/useSearch.ts';
import Button from '../ui/Button.vue';
import {
  ArrowDownAZ,
  ArrowDownWideNarrow,
  ArrowUpWideNarrow,
  ArrowDownZA,
  Star,
  Grid2X2,
  KeyRound,
  Plus
} from '@lucide/vue';
import useAppStore from '../../stores/appStore.ts';
import Select from '../ui/Select.vue';
import { useModal } from '../../composables/useModal.ts';
import NewAccountModal from '../ui/NewAccountModal.vue';

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

const { searchQuery } = useSearch();
const { state } = useAppStore();
const { getAllAccounts } = useVault();
const { openModal } = useModal();

const accounts = ref<Account[]>([]);
const sortCategory = ref<SortCategory>('alphabetical');
const sortAsc = ref(sortCategory.value === 'alphabetical');

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

const categories = [
  { value: 'date', label: 'Date Updated' },
  { value: 'alphabetical', label: 'Alphabetical' }
];

const filteredAccounts = computed(() => {
  let list = [...accounts.value];

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

    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(account);
  }

  return Array.from(groups.entries()).map(([label, items]) => ({ label, items }));
});

function setSortCategory(category: SortCategory) {
  sortCategory.value = category;
  sortAsc.value = category === 'alphabetical';
}

function toggleSort() {
  sortAsc.value = !sortAsc.value;
}

function openCreateAccountModal() {
  openModal(NewAccountModal, {
    onClose: () => {},
    onConfirm: () => {}
  });
}

// Update links to preserve current view context and clear 'edit'/'create' modes
function accountRoute(accountId: string) {
  return {
    name: route.name as string,
    params: {
      ...route.params,
      accountId: accountId,
      mode: undefined // Clear edit/create mode when switching accounts
    },
    query: route.query // Preserve filter queries
  };
}

// Reload accounts whenever the filter or search changes
watch(
  () => [
    props.vault_id,
    props.favourite_only,
    props.tags,
    props.recently_accessed,
    searchQuery.value,
    state.mutationCount,
    state.vaultMutationCount
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

// Reset sort defaults when view changes
watch(
  () => [props.vault_id, props.favourite_only, props.recently_accessed],
  () => {
    sortCategory.value = props.recently_accessed ? 'date' : 'alphabetical';
    sortAsc.value = sortCategory.value === 'alphabetical';
  },
  { immediate: true }
);
</script>

<template>
  <div class="container">
    <div class="filter-wrapper">
      <Select
        :disabled="groupedAccounts.length === 0"
        :model-value="sortCategory"
        :options="categories"
        variant="label"
        :icon-component="Grid2X2"
        :icon-props="{
          color: 'var(--color-accent)'
        }"
        @update:model-value="setSortCategory($event as SortCategory)"
      />

      <Button
        @click="toggleSort"
        :disabled="groupedAccounts.length === 0"
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
      />
    </div>

    <div class="results-wrapper no-scrollbar">
      <div v-if="groupedAccounts.length === 0" class="empty-state">
        <h3>No accounts found</h3>

        <p>Add your first account to get started.</p>
        <Button variant="accent" :icon-component="Plus" @click="openCreateAccountModal">
          New Account
        </Button>
      </div>

      <div v-for="group in groupedAccounts" class="account-group" :key="group.label">
        <h2>{{ group.label }}</h2>
        <ul class="account-list">
          <li v-for="account in group.items" :key="account.id">
            <!-- active-class works natively here because router-link matches the accountId param -->
            <RouterLink class="account" :to="accountRoute(account.id)" active-class="active">
              <div class="account-icon">
                <img
                  v-if="account.icon"
                  :src="account.icon"
                  alt="Account icon"
                  class="icon-image"
                />
                <span v-else class="icon-text">
                  {{ (account.display_name || account.username)[0].toUpperCase() }}
                </span>
                <Star
                  v-if="account.favourite"
                  :size="20"
                  :fill="account.favourite ? 'var(--color-accent)' : undefined"
                />
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

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;
  gap: 0.75rem;
  height: 100%;
  padding: 3rem 1.5rem;
  text-align: center;

  padding-bottom: 5.5rem;

  > h3 {
    font-family: var(--font-geo);
    font-size: 1.125rem;
    font-weight: 550;
    color: var(--color-text);
  }

  > p {
    font-size: 0.875rem;
    color: var(--color-text-tertiary);
    max-width: 20rem;
    margin-bottom: 0.5rem;
  }
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
  position: relative;

  font-size: 1.25rem;
  font-family: var(--font-geo);
  font-weight: 500;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);

  border-radius: 0.375rem;
  box-shadow: var(--shadow-sm);

  .icon-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 0.375rem;
  }

  .icon-text {
    pointer-events: none;
  }

  > svg {
    position: absolute;
    right: -0.6rem;
    bottom: -0.2rem;
  }
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
