<script setup lang="ts">
import { useRoute } from 'vue-router';
import { computed, watch } from 'vue';
import AccountDetails from './AccountDetails.vue';
import AccountEdit from './AccountEdit.vue';
import NoteDetails from './NoteDetails.vue';
import NoteEdit from './NoteEdit.vue';
import useAppStore from '../../stores/appStore.ts';

const route = useRoute();
const { state, setActiveAccount, setActiveItem } = useAppStore();

const itemId = computed(() => route.params.accountId as string | undefined);
const mode = computed(() => route.params.mode as string | undefined);

const isCreate = computed(() => itemId.value === 'create' || mode.value === 'create');
const isEdit = computed(() => mode.value === 'edit' && !isCreate.value);

// True when the active item is a note kind (panel switches to note views)
const isNote = computed(() => state.activeItem?.kind === 'note');

watch(
  itemId,
  (id) => {
    // Fetch both surfaces for the id: the generic item view and (for
    // accounts) the account view. Whichever kind matches is used.
    if (id && !isCreate.value) {
      setActiveItem(id);
      setActiveAccount(id);
    } else {
      setActiveItem(null);
      setActiveAccount(null);
    }
  },
  { immediate: true }
);
</script>

<template>
  <!--
    Per-kind panels: notes get their own display/edit components;
    accounts keep the existing flow until the full rename.
  -->
  <NoteEdit v-if="isNote && (isCreate || isEdit)" />

  <NoteDetails v-else-if="isNote && itemId" />

  <AccountEdit v-else-if="!isNote && (isCreate || isEdit)" />

  <!-- Details view -->
  <AccountDetails v-else-if="!isNote && itemId" />

  <!-- Empty state when no item is clicked -->
  <div v-else class="empty-panel">
    <p>Select an item to view details</p>
  </div>
</template>

<style scoped>
.empty-panel {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 100%;
  color: var(--color-text-tertiary);
  flex: 1;
}
</style>
