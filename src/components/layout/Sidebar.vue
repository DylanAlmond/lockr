<script setup lang="ts">
import { RouterLink, useRoute, useRouter } from 'vue-router';
import Logo from '../../assets/logo-text.svg';
import { ChevronUp, Clock, EllipsisVertical, KeyRound, Lock, Plus, Star } from '@lucide/vue';
import { useUser } from '../../composables/useUser';
import Button from '../ui/Button.vue';
import Dropdown, { DropdownItem } from '../ui/Dropdown.vue';
import { useVault } from '../../composables/useVault';
import { markRaw, onMounted, ref, watch } from 'vue';
import { Vault } from '../../types/index.ts';
import { useModal } from '../../composables/useModal.ts';
import VaultModal from '../ui/VaultModal.vue';
import AlertModal from '../ui/AlertModal.vue';
import useAppStore from '../../stores/appStore.ts';
import AboutAppModal from '../ui/AboutAppModal.vue';

const route = useRoute();
const router = useRouter();
const { user } = useUser();
const { getUnlockedVaults } = useVault();
const { openModal } = useModal();
const { state, deleteVaultById } = useAppStore();

const vaults = ref<Vault[]>([]);

const navItems = [
  { icon: KeyRound, title: 'All Items', filter: undefined },
  { icon: Star, title: 'Favourites', filter: 'favourites' },
  { icon: Clock, title: 'Recently Accessed', filter: 'recently-accessed' }
];

const topMenu: DropdownItem[] = [
  {
    label: 'Settings',
    onSelect: () => router.push({ name: 'settings' })
  },
  {
    label: 'About',
    onSelect: () => {
      openModal(markRaw(AboutAppModal), { onClose: () => {}, onConfirm: () => {} });
    }
  }
];

// Helper to determine if a nav item is strictly active based on the query parameter
function isNavActive(filter: string | undefined) {
  return route.name === 'all-items' && (route.query.filter as string | undefined) === filter;
}

async function refreshVaults() {
  vaults.value = await getUnlockedVaults();
}

function openCreateVaultModal(event: Event) {
  event.preventDefault();
  event.stopPropagation();

  openModal(markRaw(VaultModal), {
    vault: null,
    onClose: () => {},
    onConfirm: () => {}
  });
}

async function handleDeleteVault(vault: Vault) {
  const success = await deleteVaultById(vault.id);

  if (success && route.params.vaultId === vault.id) {
    router.replace({ name: 'all-items' });
  }
}

function vaultMenuItems(vault: Vault): DropdownItem[] {
  return [
    {
      label: 'Edit Vault',
      onSelect: () => {
        openModal(markRaw(VaultModal), {
          vault,
          onClose: () => {},
          onConfirm: () => {}
        });
      }
    },
    {
      label: 'Delete Vault',
      onSelect: () => {
        openModal(markRaw(AlertModal), {
          title: `Delete "${vault.name}"`,
          message:
            "Are you sure you want to continue? All accounts in this vault will be deleted. This can't be undone.",
          actionLabel: 'Delete',
          confirmationValue: vault.name,
          onClose: () => {},
          onConfirm: () => handleDeleteVault(vault)
        });
      }
    }
  ];
}

watch(() => state.vaultMutationCount, refreshVaults);

onMounted(refreshVaults);
</script>

<template>
  <header class="sidebar">
    <div class="logo-container" data-tauri-drag-region>
      <Logo />

      <Dropdown :list="topMenu" #trigger="{ triggerProps }">
        <Button
          aria-label="Vault Options"
          icon-only
          variant="label"
          size="small"
          :icon-component="EllipsisVertical"
          v-bind="triggerProps"
        />
      </Dropdown>
    </div>

    <div class="user-profile">
      <RouterLink
        to="/settings"
        class="user-profile-link"
        aria-label="Settings"
        :class="{ active: route.path.startsWith('/settings') }"
      >
        <div class="user-icon">
          <img v-if="user?.icon" :src="user.icon" alt="User icon" class="icon-image" />
          <span v-else class="icon-text"> {{ (user?.name || 'No User ')[0] }} </span>
        </div>

        <span class="user-name">{{ user?.name || 'No User' }}</span>
      </RouterLink>
    </div>

    <nav class="sidebar-nav">
      <ul class="link-list">
        <li v-for="item in navItems" :key="item.title">
          <RouterLink
            :to="{ name: 'all-items', query: item.filter ? { filter: item.filter } : {} }"
            class="nav-link"
            :class="{ active: isNavActive(item.filter) }"
          >
            <component :is="item.icon" :size="20" aria-hidden="true" />
            <span>{{ item.title }}</span>
          </RouterLink>
        </li>
      </ul>

      <details class="vaults-container" open>
        <summary class="vaults-accordion">
          <ChevronUp class="vaults-chevron" :size="20" />
          <span>Vaults</span>
          <Button
            :icon-component="Plus"
            variant="label"
            size="small"
            icon-only
            aria-label="New Vault"
            @click="openCreateVaultModal"
          />
        </summary>

        <ul class="link-list">
          <li v-for="vault in vaults" :key="vault.id" class="vault-item">
            <RouterLink
              :to="{ name: 'vault', params: { vaultId: vault.id } }"
              class="nav-link"
              active-class="active"
            >
              <Lock :size="20" aria-hidden="true" :color="vault.color" />
              <span>{{ vault.name }}</span>
            </RouterLink>

            <Dropdown class="vault-menu" :list="vaultMenuItems(vault)" #trigger="{ triggerProps }">
              <Button
                aria-label="Vault Options"
                icon-only
                variant="label"
                size="small"
                :icon-component="EllipsisVertical"
                v-bind="triggerProps"
              />
            </Dropdown>
          </li>
        </ul>
      </details>
    </nav>
  </header>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;

  width: 320px;
  height: 100%;

  background: var(--color-bg-nav);

  border-right: 1px solid var(--color-border);
}

.logo-container {
  display: flex;
  align-items: center;
  justify-content: space-between;
  box-sizing: border-box;
  width: 100%;
  height: 4rem;

  padding: 0.75rem 1rem;
}

.user-profile {
  padding-left: 0.75rem;
  padding-right: 0.75rem;
  padding-top: 0.625rem;
  padding-bottom: 0.25rem;
}

.user-profile-link {
  display: flex;
  align-items: center;
  gap: 0.75rem;

  width: 100%;
  padding: 0.5rem;
  box-sizing: border-box;

  text-decoration: none;
  color: inherit;
  cursor: pointer;

  border-radius: 0.75rem;
  transition: background-color 0.2s ease;

  &:hover,
  &.active {
    background-color: var(--color-hover);
  }

  &.active {
    box-shadow: var(--inset-sm);
  }

  &:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}

.user-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 3rem;
  height: 3rem;
  aspect-ratio: 1/1;

  font-size: 1.25rem;
  font-family: var(--font-geo);
  font-weight: 500;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);

  border-radius: 0.375rem;

  .icon-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 0.375rem;
  }

  .icon-text {
    pointer-events: none;
  }
}

.user-name {
  font-size: 1.25rem;
  font-family: var(--font-geo);
  font-weight: 500;
}

.sidebar-nav {
  display: flex;
  flex-direction: column;
  gap: 2rem;
  padding: 1rem;
}

.link-list {
  display: flex;
  flex-direction: column;
  list-style: none;
}

.vault-item {
  display: flex;
  align-items: center;
  gap: 0.25rem;
  border-radius: 0.75rem;

  transition:
    background-color 0.2s ease,
    color 0.2s ease;

  .nav-link {
    flex: 1;
    min-width: 0;

    /* Row-level hover/active owns the background; the link itself stays transparent */
    &:hover,
    &.active {
      background-color: transparent;
      box-shadow: none;
      color: inherit;
    }

    span {
      overflow-y: visible;
      white-space: nowrap;
      text-overflow: ellipsis;
    }
  }

  &:hover,
  &:focus-within,
  &:has(.nav-link.active) {
    background-color: var(--color-hover);
  }

  &:has(.nav-link.active) {
    box-shadow: var(--inset-sm);
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }

  &:deep(.vault-menu) {
    opacity: 0;
    transition: opacity 0.15s ease;
  }

  @media (hover: hover) {
    &:has(.nav-link.active):deep(.vault-menu),
    &:hover :deep(.vault-menu),
    &:focus-within :deep(.vault-menu) {
      opacity: 1;
    }
  }
}

.vault-item :deep(.vault-menu) {
  flex-shrink: 0;
  margin-right: 0.5rem;
}

.vaults-container[open] .vaults-chevron {
  transform: rotate(180deg);
}

.vaults-chevron {
  transition: transform 0.25s ease;
}

.vaults-accordion {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 0.5rem 1rem;
  padding-right: 0.5rem;
  box-sizing: border-box;
  cursor: pointer;

  height: 2.625rem;
  border-radius: 0.375rem;

  transition: box-shadow 0.2s ease;

  > svg {
    color: var(--color-text-tertiary);
  }

  button {
    margin-left: auto;
  }

  /* Hide default marker */
  &::-webkit-details-marker {
    display: none;
  }

  &:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  /* Corner smoothing */
  @supports (corner-shape: squircle) {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}

.nav-link {
  appearance: none;
  border: none;
  outline: none;

  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  gap: 0.75rem;

  width: 100%;
  height: 3rem;
  padding: 0.5rem 1rem;
  box-sizing: border-box;

  position: relative;
  overflow: hidden;
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

  & > svg {
    color: var(--color-accent-muted);
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
</style>
