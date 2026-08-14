<script setup lang="ts">
import {
  ArrowLeft,
  ArrowRight,
  ChevronLeft,
  ChevronRight,
  KeyRound,
  Plus,
  Search,
  Vault
} from '@lucide/vue';
import Input from '../ui/Input.vue';
import AppShell from './AppShell.vue';
import Button from '../ui/Button.vue';
import { useRouterHistory } from '../../composables/useRouterHistory.ts';
import { useRoute, useRouter } from 'vue-router';
import { useSearch } from '../../composables/useSearch.ts';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import { ref, computed, markRaw, watch } from 'vue';
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

const hasSelectedItem = computed(() => !!route.params.accountId);

const isPanelOpen = ref(hasSelectedItem.value);

const togglePanel = () => {
  isPanelOpen.value = !isPanelOpen.value;
};

watch(
  () => route.params.accountId,
  (accountId) => {
    isPanelOpen.value = !!accountId;
  }
);

const createMenuItems = ref<DropdownItem[]>([
  {
    label: 'New Vault',
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
          Add
        </Button>
      </Dropdown>
    </template>

    <div
      class="accounts-content"
      :class="{
        'has-selection': hasSelectedItem,
        'panel-open': isPanelOpen
      }"
    >
      <main>
        <router-view name="list" />
      </main>

      <aside :class="{ 'is-open': isPanelOpen }">
        <Button
          v-if="hasSelectedItem"
          class="panel-toggle"
          name="toggle-panel"
          :aria-label="isPanelOpen ? 'Hide details' : 'Show details'"
          variant="outline"
          :icon-component="isPanelOpen ? ChevronRight : ChevronLeft"
          icon-only
          @click="togglePanel"
        />

        <div class="panel-content">
          <router-view name="panel" />
        </div>
      </aside>
    </div>
  </AppShell>
</template>

<style scoped>
.accounts-content {
  position: relative;
  display: grid;
  grid-template-columns: 440px minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr);

  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.accounts-content > main {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.accounts-content > aside {
  position: relative;

  display: flex;
  flex-direction: column;

  min-width: 0;
  min-height: 0;

  background: var(--color-bg);
  border-left: 1px solid var(--color-border);
}

.panel-content {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
  overflow: auto;
}

.panel-toggle {
  display: none;

  position: absolute;
  top: 50%;
  left: 0;

  width: 28px;
  height: 48px;

  transform: translate(-100%, -100%);

  align-items: center;
  justify-content: center;
  z-index: 11;

  background-color: var(--color-bg);

  border-top-right-radius: 0;
  border-bottom-right-radius: 0;
}

/*
 * Desktop:
 * The panel is part of the normal layout.
 */
.accounts-content.has-selection {
  grid-template-columns: minmax(360px, 440px) minmax(480px, 1fr);
}

/*
 * Small desktop:
 * The panel becomes a slide-over.
 */
@media (max-width: 1140px) {
  .accounts-content {
    display: block;
  }

  .accounts-content > main {
    width: 100%;
    height: 100%;
  }

  .accounts-content > aside {
    position: absolute;
    inset: 0 0 0 auto;

    width: fit-content;
    height: 100%;

    z-index: 10;

    background: var(--color-bg);
    border-left: 1px solid var(--color-border);

    box-shadow: -8px 0 24px rgb(0 0 0 / 0.12);

    transform: translateX(100%);
    transition: transform 0.5s cubic-bezier(0.22, 1, 0.36, 1);
  }

  .accounts-content > aside.is-open {
    transform: translateX(0);
  }

  .panel-toggle {
    display: flex;
  }
}

.nav-button-container {
  display: flex;
  flex-shrink: 0;
}

.nav-search {
  min-width: 0;
  flex-shrink: 1;
}
</style>
