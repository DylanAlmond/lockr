<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { Account } from '../../types';
import { useRoute, useRouter } from 'vue-router';
import { useVault } from '../../composables/useVault';

const { getAllAccounts } = useVault();
const route = useRoute();
const router = useRouter();

const accounts = ref<Account[]>([]);
const selectedTag = ref<string>('All');
const sortAsc = ref<boolean>(false); // false = newest first (descending)

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

async function loadAccounts(id: string) {
  accounts.value = await getAllAccounts({ vault_id: id });

  // Navigate to first result
  router.push({
    path: `/vaults/${route.params.vaultId as string}/${accounts.value[0].id}`,
    replace: true
  });
}

// Build tag options from loaded accounts
const tagOptions = computed(() => {
  const tagSet = new Set<string>();
  accounts.value.forEach((a) => a.tags.forEach((t) => tagSet.add(t)));
  return ['All', ...Array.from(tagSet).sort()];
});

// Filtered + sorted accounts
const filteredAccounts = computed(() => {
  let list =
    selectedTag.value === 'All'
      ? [...accounts.value]
      : accounts.value.filter((a) => a.tags.includes(selectedTag.value));

  list.sort((a, b) => {
    const dateA = new Date(a.updated_at).getTime();
    const dateB = new Date(b.updated_at).getTime();
    return sortAsc.value ? dateA - dateB : dateB - dateA;
  });

  return list;
});

// Group by month
interface MonthGroup {
  label: string;
  items: Account[];
}

const groupedAccounts = computed<MonthGroup[]>(() => {
  const map = new Map<string, Account[]>();

  for (const account of filteredAccounts.value) {
    const d = new Date(account.updated_at);
    const label = `${monthNames[d.getMonth()]} ${d.getFullYear()}`;
    if (!map.has(label)) map.set(label, []);
    map.get(label)!.push(account);
  }

  return Array.from(map.entries()).map(([label, items]) => ({ label, items }));
});

function selectTag(tag: string) {
  selectedTag.value = tag;
}

function toggleSort() {
  sortAsc.value = !sortAsc.value;
}

watch(() => route.params.vaultId as string, loadAccounts, { immediate: true });
</script>

<template>
  <div class="container">
    <div class="filter-wrapper">
      <select :value="selectedTag" @change="selectTag(($event.target as HTMLSelectElement).value)">
        <option v-for="tag in tagOptions" :key="tag" :value="tag">
          {{ tag }}
        </option>
      </select>

      <button @click="toggleSort">
        {{ sortAsc ? 'Oldest First' : 'Newest First' }}
      </button>
    </div>

    <div class="results-wrapper no-scrollbar">
      <div v-if="groupedAccounts.length === 0">
        <p>No accounts found.</p>
      </div>

      <div v-for="group in groupedAccounts" class="account-group" :key="group.label">
        <h2>{{ group.label }}</h2>

        <ul class="account-list">
          <li v-for="account in group.items" :key="account.id">
            <RouterLink
              class="account"
              :to="{ path: `/vaults/${route.params.vaultId as string}/${account.id}` }"
              active-class="active"
            >
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
  padding-top: 3rem;
  overflow-y: auto;
}

.account-group {
  display: flex;
  flex-direction: column;
  gap: 1rem;

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
    margin-bottom: 0.25rem;
  }

  > span {
    color: var(--color-text-tertiary);
    font-size: 0.875rem;
  }
}
</style>
