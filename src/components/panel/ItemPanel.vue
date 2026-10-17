<script setup lang="ts">
import { useRoute } from 'vue-router';
import { computed, watch } from 'vue';
import AccountDetails from './AccountDetails.vue';
import AccountEdit from './AccountEdit.vue';
import CardDetails from './CardDetails.vue';
import CardEdit from './CardEdit.vue';
import IdentityDetails from './IdentityDetails.vue';
import IdentityEdit from './IdentityEdit.vue';
import NoteDetails from './NoteDetails.vue';
import NoteEdit from './NoteEdit.vue';
import useAppStore from '../../stores/appStore.ts';

const route = useRoute();
const { state, setActiveItem } = useAppStore();

const itemId = computed(() => route.params.itemId as string | undefined);
const mode = computed(() => route.params.mode as string | undefined);

const isCreate = computed(() => itemId.value === 'create' || mode.value === 'create');
const isEdit = computed(() => mode.value === 'edit' && !isCreate.value);

// Dispatch by the active item's kind
const isNote = computed(() => state.activeItem?.kind === 'note');
const isIdentity = computed(() => state.activeItem?.kind === 'identity');
const isCard = computed(() => state.activeItem?.kind === 'card');
const isAccount = computed(() => state.activeItem?.kind === 'account');

watch(
  itemId,
  (id) => {
    if (id && !isCreate.value) {
      setActiveItem(id);
    } else {
      setActiveItem(null);
    }
  },
  { immediate: true }
);
</script>

<template>
  <!-- Per-kind panels: each kind gets its own display/edit components -->
  <NoteEdit v-if="isNote && (isCreate || isEdit)" />

  <NoteDetails v-else-if="isNote && itemId" />

  <IdentityEdit v-else-if="isIdentity && (isCreate || isEdit)" />

  <IdentityDetails v-else-if="isIdentity && itemId" />

  <CardEdit v-else-if="isCard && (isCreate || isEdit)" />

  <CardDetails v-else-if="isCard && itemId" />

  <AccountEdit v-else-if="isAccount && (isCreate || isEdit)" />

  <!-- Details view -->
  <AccountDetails v-else-if="isAccount && itemId" />

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
