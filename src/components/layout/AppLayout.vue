<script setup lang="ts">
import { ArrowLeft, ArrowRight, KeyRound, Plus, Search, Vault } from '@lucide/vue';
import Input from '../ui/Input.vue';
import AppShell from './AppShell.vue';
import Button from '../ui/Button.vue';
import { useRouterHistory } from '../../composables/useRouterHistory.ts';
import { useRoute, useRouter } from 'vue-router';
import { useSearch } from '../../composables/useSearch.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import { ref, computed, markRaw } from 'vue';
import { useModal } from '../../composables/useModal.ts';
import NewAccountModal from '../ui/NewAccountModal.vue';
import VaultModal from '../ui/VaultModal.vue';

const route = useRoute();
const router = useRouter();
const { canGoBack, canGoForward, goBack, goForward } = useRouterHistory(router);
const { searchQuery } = useSearch();
const { openModal } = useModal();

// Derive edit state directly from the URL
const isEditing = computed(() => route.params.mode === 'edit');

const createMenuItems = ref<DropdownItem[]>([
  {
    label: 'Vault',
    icon: Vault,
    onSelect: () => {
      openModal(markRaw(VaultModal), {
        vault: null,
        onClose: () => {},
        onConfirm: () => {}
      });
    }
  },
  {
    label: 'New Account',
    icon: KeyRound,
    onSelect: () => {
      openModal(NewAccountModal, {
        onClose: () => {},
        onConfirm: () => {}
      });
    }
  }
]);
</script>

<template>
  <AppShell>
    <template #titlebar>
      <div class="nav-button-container">
        <Button
          name="navigate-back"
          aria-label="Back"
          variant="label"
          :icon-component="ArrowLeft"
          icon-only
          :disabled="!canGoBack"
          @click="goBack"
        />
        <Button
          name="navigate-forward"
          aria-label="Forward"
          variant="label"
          :icon-component="ArrowRight"
          icon-only
          :disabled="!canGoForward"
          @click="goForward"
        />
      </div>

      <Input
        :disabled="isEditing"
        class="nav-search"
        name="nav-search"
        type="search"
        v-model="searchQuery"
        :icon-component="Search"
        placeholder="Search..."
      />

      <Dropdown :list="createMenuItems" #trigger="{ triggerProps }">
        <Button
          name="Create"
          variant="accent"
          :icon-component="Plus"
          :disabled="isEditing"
          v-bind="triggerProps"
        >
          Create
        </Button>
      </Dropdown>
    </template>

    <div class="accounts-content">
      <main>
        <router-view name="list" />
      </main>

      <aside>
        <router-view name="panel" />
      </aside>
    </div>
  </AppShell>
</template>

<style scoped>
.accounts-content {
  display: grid;
  grid-template-columns: 440px 1fr;
  grid-template-rows: minmax(0, 1fr);
  flex: 1;
  min-height: 0;

  > main {
    display: flex;
    flex-direction: column;
  }

  > aside {
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--color-border);
  }
}

.nav-button-container {
  flex-shrink: 0;
}

.nav-search {
  flex-shrink: 1;
}
</style>
