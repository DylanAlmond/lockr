<script setup lang="ts">
import { RouterLink } from 'vue-router';
import Logo from '../../assets/logo-text.svg';
import { ChevronUp, Clock, KeyRound, Lock, Plus, Star } from '@lucide/vue';
import { useUser } from '../../composables/useUser';
import Button from '../ui/Button.vue';
import { useVault } from '../../composables/useVault';

const { user } = useUser();
const { unlockedVaults } = useVault();

const navItems = [
  { icon: KeyRound, title: 'All Items', route: '/all-items' },
  { icon: Star, title: 'Favourites', route: '/favourites' },
  { icon: Clock, title: 'Recently Accessed', route: '/recently-accessed' }
  // { icon: Lock, title: 'Vaults', route: '/vaults' }
];
</script>

<template>
  <header class="sidebar">
    <div class="logo-container" data-tauri-drag-region>
      <Logo />
    </div>

    <div class="user-profile">
      <span class="user-icon">{{ (user?.name || 'No User ')[0] }}</span>
      <span class="user-name">{{ user?.name || 'No User' }}</span>
    </div>

    <nav class="sidebar-nav">
      <ul class="link-list">
        <li v-for="item in navItems">
          <RouterLink :to="{ path: item.route }" class="nav-link" active-class="active">
            <component :is="item.icon" :size="20" aria-hidden="true" />
            <span>{{ item.title }}</span>
          </RouterLink>
        </li>
      </ul>

      <details class="vaults-container" open>
        <summary class="vaults-accordion">
          <ChevronUp class="vaults-chevron" :size="20" />
          <span>Vaults</span>
          <Button :icon-component="Plus" variant="label" size="small" icon-only />
        </summary>

        <ul class="link-list">
          <li v-for="vault in unlockedVaults">
            <RouterLink :to="{ path: `/vault/${vault.id}` }" class="nav-link" active-class="active">
              <Lock :size="20" aria-hidden="true" :color="vault.color" />
              <span>{{ vault.name }}</span>
            </RouterLink>
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
  box-sizing: border-box;
  width: 100%;
  height: 4rem;

  padding: 0.75rem 1rem;
}

.user-profile {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding-left: 1.25rem;
  padding-right: 1.25rem;
  padding-top: 0.625rem;
  padding-bottom: 1rem;
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
    outline: 2px solid var(--color-accent);
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
    background-color: var(--color-accent-hover);
    color: var(--color-accent-dark);
  }

  & > svg {
    color: var(--color-accent-muted);
  }

  &:focus-visible {
    outline: 2px solid var(--color-accent);
    outline-offset: 2px;
  }

  &:hover {
    background-color: var(--color-accent-hover);
    color: var(--color-accent-dark);
  }

  /* Corner smoothing */
  @supports (corner-shape: squircle) {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}
</style>
