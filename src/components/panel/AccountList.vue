<script setup lang="ts">
import { ref, computed, watch } from 'vue';
import { SafeItem, AccountFilter } from '../../types/index.ts';
import { useRoute } from 'vue-router';
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
  Plus
} from '@lucide/vue';
import useAppStore from '../../stores/appStore.ts';
import Select from '../ui/Select.vue';
import { useModal } from '../../composables/useModal.ts';
import NewItemModal from '../ui/NewItemModal.vue';

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
const { getAllItems } = useVault();
const { openModal } = useModal();

// Kind-aware display helpers shared by grouping, sorting and row rendering
function itemLabel(item: SafeItem): string {
  if (item.display_name) return item.display_name;
  if (item.kind === 'account') return item.username;
  if (item.kind === 'identity') return item.full_name;
  return item.title || 'Untitled';
}

// Single-line snippet of a note's body for the row subtitle
function noteSnippet(body: string): string {
  const firstLine = body.split('\n')[0].trim();
  return firstLine.length > 60 ? `${firstLine.slice(0, 60)}…` : firstLine;
}

function itemSubtitle(item: SafeItem): string {
  if (item.kind === 'account') return item.email || item.username;
  if (item.kind === 'identity') return item.email || item.phone || item.full_name;
  return noteSnippet(item.body) || item.title || '';
}

const items = ref<SafeItem[]>([]);
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

const filteredItems = computed(() => {
  let list = [...items.value];

  list.sort((a, b) => {
    let comparison = 0;
    if (sortCategory.value === 'date') {
      comparison = new Date(a.updated_at).getTime() - new Date(b.updated_at).getTime();
    }
    if (sortCategory.value === 'alphabetical') {
      comparison = itemLabel(a).localeCompare(itemLabel(b));
    }
    return sortAsc.value ? comparison : -comparison;
  });

  return list;
});

interface ItemGroup {
  label: string;
  items: SafeItem[];
}

const groupedItems = computed<ItemGroup[]>(() => {
  const groups = new Map<string, SafeItem[]>();

  for (const item of filteredItems.value) {
    let label: string;
    if (sortCategory.value === 'date') {
      const d = new Date(item.updated_at);
      label = `${monthNames[d.getMonth()]} ${d.getFullYear()}`;
    } else {
      label = itemLabel(item)[0].toUpperCase();
    }

    if (!groups.has(label)) groups.set(label, []);
    groups.get(label)!.push(item);
  }

  return Array.from(groups.entries()).map(([label, list]) => ({ label, items: list }));
});

function setSortCategory(category: SortCategory) {
  sortCategory.value = category;
  sortAsc.value = category === 'alphabetical';
}

function toggleSort() {
  sortAsc.value = !sortAsc.value;
}

function openCreateItemModal() {
  openModal(NewItemModal, {
    onClose: () => {}
  });
}

// Update links to preserve current view context and clear 'edit'/'create' modes
function itemRoute(itemId: string) {
  return {
    name: route.name as string,
    params: {
      ...route.params,
      accountId: itemId,
      mode: undefined // Clear edit/create mode when switching items
    },
    query: route.query // Preserve filter queries
  };
}

// Reload items whenever the filter or search changes
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
    const nextItems = await getAllItems({
      ...props,
      search_query: searchQuery.value
    });
    items.value = nextItems;
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
        :disabled="groupedItems.length === 0"
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
        :disabled="groupedItems.length === 0"
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
      <div v-if="groupedItems.length === 0" class="empty-state">
        <h3>No items found</h3>

        <p>Add your first item to get started.</p>
        <Button variant="accent" :icon-component="Plus" @click="openCreateItemModal">
          New Item
        </Button>
      </div>

      <div v-for="group in groupedItems" class="account-group" :key="group.label">
        <h2>{{ group.label }}</h2>
        <ul class="account-list">
          <li v-for="item in group.items" :key="item.id">
            <!-- active-class works natively here because router-link matches the accountId param -->
            <RouterLink class="account" :to="itemRoute(item.id)" active-class="active">
              <div class="account-icon">
                <img v-if="item.icon" :src="item.icon" alt="Item icon" class="icon-image" />
                <span v-else class="icon-text">
                  {{ itemLabel(item)[0].toUpperCase() }}
                </span>
                <Star
                  v-if="item.favourite"
                  :size="20"
                  :fill="item.favourite ? 'var(--color-accent)' : 'none'"
                  color="var(--color-accent)"
                />
              </div>
              <div class="account-meta">
                <h3>{{ itemLabel(item) }}</h3>
                <span>{{ itemSubtitle(item) }}</span>
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
    background-color: var(--color-hover);
  }

  &:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--color-accent);
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
