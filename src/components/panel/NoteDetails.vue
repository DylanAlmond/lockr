<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight, EllipsisVertical, Pencil, Star } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import { VAULT_COLORS } from '../../util/constants.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import AlertModal from '../ui/AlertModal.vue';
import MoveAccountModal from '../ui/MoveAccountModal.vue';
import { useModal } from '../../composables/useModal.ts';
import { useVault } from '../../composables/useVault.ts';
import { markRaw } from 'vue';
import useAppStore from '../../stores/appStore.ts';
import { Vault } from '../../types/index.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem, deleteActiveItem } = useAppStore();
const { getVaultById } = useVault();
const { openModal } = useModal();

const parentVault = ref<Vault | null>(null);

const note = computed(() =>
  state.activeItem?.kind === 'note' ? state.activeItem : null
);

const displayLabel = computed(() => note.value?.display_name || note.value?.title || 'Untitled');

const displayInitial = computed(() => displayLabel.value[0].toUpperCase());

async function toggleFavourite() {
  if (!note.value) return;
  updateActiveItem({ favourite: !note.value.favourite });
}

function goToEdit() {
  if (!note.value) return;
  router.push({
    name: route.name as string,
    params: {
      ...route.params,
      accountId: note.value.id,
      mode: 'edit'
    },
    query: route.query
  });
}

async function handleDelete() {
  if (!note.value) return;

  const success = await deleteActiveItem();

  if (success) {
    await router.replace({
      name: route.name as string,
      params: {
        ...route.params,
        accountId: undefined,
        mode: undefined
      },
      query: route.query
    });
  }
}

const miscMenuItems = computed<DropdownItem[]>(() => [
  {
    label: 'Move to Vault',
    disabled: !note.value,
    onSelect: () => {
      if (!note.value) return;
      openModal(markRaw(MoveAccountModal), {
        onClose: () => {},
        onConfirm: () => {}
      });
    }
  },
  {
    label: 'Delete Note',
    onSelect: () => {
      openModal(markRaw(AlertModal), {
        title:
          'Delete ' +
          (note.value?.display_name || note.value?.title
            ? `"${displayLabel.value}"`
            : 'Note'),
        message: "Are you sure you want to continue? This can't be undone.",
        actionLabel: 'Delete',
        confirmationValue: note.value?.display_name || note.value?.title || undefined,
        onClose: () => {},
        onConfirm: () => handleDelete()
      });
    }
  }
]);

// Clear parent vault when the active item changes
watch(
  () => [state.activeItem?.id, state.activeItem?.vault_id],
  async () => {
    if (note.value) {
      parentVault.value = await getVaultById(note.value.vault_id);
    } else {
      parentVault.value = null;
    }
  },
  { immediate: true }
);
</script>

<template>
  <!-- Empty -->
  <div v-if="!note" class="wrapper">No note found.</div>

  <!-- View Mode -->
  <div v-else class="wrapper">
    <header>
      <div class="vault-label">
        <span
          class="vault-indicator"
          :style="{ backgroundColor: parentVault?.color || VAULT_COLORS[0].hex }"
          aria-hidden="true"
        ></span>
        <span class="vault-name">{{ parentVault?.name || 'Unknown Vault' }}</span>
      </div>

      <nav class="header-toolbar">
        <Button
          aria-label="Favourite"
          icon-only
          variant="outline"
          size="small"
          :icon-component="Star"
          :icon-props="{
            fill: note.favourite ? 'var(--color-accent)' : 'none',
            color: note.favourite ? 'var(--color-accent)' : undefined
          }"
          @click="toggleFavourite"
        />

        <Button variant="outline" size="small" :icon-component="Pencil" @click="goToEdit">
          Edit
        </Button>

        <Dropdown :list="miscMenuItems" #trigger="{ triggerProps }">
          <Button
            class="menu-button"
            aria-label="Note Menu"
            icon-only
            variant="label"
            size="small"
            :icon-component="EllipsisVertical"
            v-bind="triggerProps"
          />
        </Dropdown>
      </nav>
    </header>

    <main class="thin-scrollbar">
      <section class="descriptor-section">
        <div class="item-icon">
          <img
            v-if="note.icon"
            :src="note.icon"
            alt="Note icon"
            class="icon-image"
          />
          <span v-else class="icon-text">
            {{ displayInitial }}
          </span>
          <Star
            v-if="note.favourite"
            :size="32"
            :fill="note.favourite ? 'var(--color-accent)' : 'none'"
            color="var(--color-accent)"
          />
        </div>
        <h1 class="display-name">{{ displayLabel }}</h1>
      </section>

      <section class="note-body-section">
        <h2>body</h2>
        <p class="note-body">{{ note.body }}</p>
      </section>

      <section class="tags-section">
        <h2>tags</h2>
        <TagList :modelValue="note.tags" />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(note.updated_at)} ` }}</span>
      </section>
    </main>
  </div>
</template>

<style scoped>
.wrapper {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  flex: 1;
}

header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1rem;
}

main {
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  gap: 1.5rem;
  padding: 1rem;
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}

.menu-button {
  --button-icon-size: 1.5rem;
}

.vault-label {
  display: flex;
  align-items: center;
  gap: 0.75rem;

  .vault-indicator {
    width: 1rem;
    height: 1rem;
    aspect-ratio: 1;
    border-radius: 0.25rem;
  }

  .vault-name {
    font-size: 1.125rem;
    font-weight: 350;
    text-box-trim: trim-both;
    text-box-edge: cap alphabetic;
  }
}

.header-toolbar {
  display: flex;
  gap: 0.75rem;
}

.descriptor-section {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 0.75rem 0rem;
}

.item-icon {
  position: relative;
  display: flex;
  justify-content: center;
  align-items: center;
  width: 5.25rem;
  height: 5.25rem;
  aspect-ratio: 1/1;
  font-size: 2rem;
  font-family: var(--font-geo);
  font-weight: 500;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);
  border-radius: 0.75rem;
  box-shadow: var(--shadow-sm);

  .icon-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 0.75rem;
  }

  .icon-text {
    pointer-events: none;
  }

  > svg {
    position: absolute;
    right: -1rem;
    bottom: -0.5rem;
  }
}

.display-name {
  font-size: 2rem;
  font-family: var(--font-geo);
}

.note-body-section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0rem 1.5rem;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }
}

.note-body {
  font: inherit;
  color: var(--color-text);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  line-height: 1.5;
}

.tags-section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0rem 1.5rem;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }
}

.timestamp-section {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 1.5rem;
  padding-right: 0rem;
  text-box-trim: trim-both;
  text-box-edge: cap alphabetic;

  > svg {
    color: var(--color-text-muted);
  }
}
</style>
