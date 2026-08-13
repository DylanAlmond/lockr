<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, nextTick, useAttrs } from 'vue';
import type { Component } from 'vue';

export interface DropdownItem {
  id?: string | number;
  label: string;
  icon?: Component;
  disabled?: boolean;
  onSelect?: () => void;
}

const props = withDefaults(
  defineProps<{
    list: DropdownItem[];
    align?: 'left' | 'right';
  }>(),
  {
    align: 'left'
  }
);

defineOptions({ inheritAttrs: false });

// Multi-root template disables auto fallthrough — forward attrs (class, etc.) to the trigger wrapper
const attrs = useAttrs();

const emit = defineEmits<{
  (e: 'select', item: DropdownItem): void;
}>();

const isOpen = ref(false);
const triggerWrapperRef = ref<HTMLElement | null>(null);
const menuRef = ref<HTMLUListElement | null>(null);
const menuStyles = ref<Record<string, string>>({});

const dropdownId = `dropdown-menu-${Math.random().toString(36).substring(2, 9)}`;
const alignClass = computed(() => `dropdown-menu--${props.align}`);

const updatePosition = () => {
  if (!triggerWrapperRef.value) return;

  const rect = triggerWrapperRef.value.getBoundingClientRect();
  const menuEl = document.getElementById(dropdownId);

  const menuWidth = menuEl ? menuEl.offsetWidth : Math.max(rect.width, 180);
  const menuHeight = menuEl ? menuEl.offsetHeight : 200;

  const styles: Record<string, string> = {
    position: 'fixed',
    minWidth: `${Math.max(rect.width, 180)}px`,
    zIndex: '9999'
  };

  let leftPos = rect.left;
  if (props.align === 'right') {
    leftPos = rect.right - menuWidth;
  }

  if (leftPos + menuWidth > window.innerWidth - 8) {
    leftPos = window.innerWidth - menuWidth - 8;
  }
  if (leftPos < 8) {
    leftPos = 8;
  }
  styles.left = `${leftPos}px`;

  const spaceBelow = window.innerHeight - rect.bottom;
  const spaceAbove = rect.top;

  if (spaceBelow < menuHeight + 8 && spaceAbove > spaceBelow) {
    styles.top = `${rect.top - menuHeight - 8}px`;
  } else {
    styles.top = `${rect.bottom + 8}px`;
  }

  menuStyles.value = styles;
};

const toggleDropdown = async (event: Event) => {
  event.stopPropagation();
  isOpen.value = !isOpen.value;
  updatePosition();

  // Focus if not mouse toggled
  if (isOpen.value && (event as MouseEvent).detail === 0) {
    focusFirstItem();
  }
};

const closeDropdown = () => {
  isOpen.value = false;
};

const focusTrigger = () => {
  triggerWrapperRef.value?.querySelector('button')?.focus();
};

const focusFirstItem = () => {
  const items = menuRef.value?.querySelectorAll<HTMLButtonElement>(
    'button[role="menuitem"]:not([disabled])'
  );
  items?.[0]?.focus({ focusVisible: true });
};

const handleSelect = (item: DropdownItem) => {
  if (item.disabled) return;
  if (item.onSelect) item.onSelect();
  emit('select', item);
  closeDropdown();
  focusTrigger(); // Return focus to trigger after selection
};

// Handle keyboard navigation INSIDE the menu
const handleMenuKeydown = (event: KeyboardEvent) => {
  if (!menuRef.value) return;

  const items = Array.from(
    menuRef.value.querySelectorAll('button[role="menuitem"]:not([disabled])')
  ) as HTMLElement[];

  if (!items.length) return;

  const currentIndex = items.indexOf(document.activeElement as HTMLElement);

  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault();
      items[(currentIndex + 1) % items.length].focus();
      break;
    case 'ArrowUp':
      event.preventDefault();
      items[(currentIndex - 1 + items.length) % items.length].focus();
      break;
    case 'Home':
      event.preventDefault();
      items[0].focus();
      break;
    case 'End':
      event.preventDefault();
      items[items.length - 1].focus();
      break;
    case 'Tab':
      // Prevent the browser from tabbing out to the body element
      event.preventDefault();
      if (event.shiftKey) {
        // Shift + Tab
        if (currentIndex === 0 || currentIndex === -1) {
          closeDropdown();
          focusTrigger();
        } else {
          items[currentIndex - 1].focus();
        }
      } else {
        // Tab
        if (currentIndex === items.length - 1 || currentIndex === -1) {
          closeDropdown();
          focusTrigger();
        } else {
          items[currentIndex + 1].focus();
        }
      }
      break;
    case 'Escape':
      event.preventDefault();
      closeDropdown();
      focusTrigger();
      break;
  }
};

// Handle keyboard navigation ON the trigger button
const handleTriggerKeydown = (event: KeyboardEvent) => {
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    if (!isOpen.value) {
      isOpen.value = true;
      nextTick(() => focusFirstItem());
    }
  }
};

const handleClickOutside = (event: MouseEvent) => {
  const target = event.target as HTMLElement;
  const isOutsideTrigger = triggerWrapperRef.value && !triggerWrapperRef.value.contains(target);
  const isOutsideMenu = !target.closest(`#${dropdownId}`);

  if (isOpen.value && isOutsideTrigger && isOutsideMenu) {
    closeDropdown();
  }
};

const handleKeydown = (event: KeyboardEvent) => {
  if (event.key === 'Escape' && isOpen.value) {
    closeDropdown();
    focusTrigger();
  }
};

const handleResizeOrScroll = () => {
  if (isOpen.value) closeDropdown();
};

onMounted(() => {
  document.addEventListener('click', handleClickOutside);
  document.addEventListener('keydown', handleKeydown);
  window.addEventListener('resize', handleResizeOrScroll);
  window.addEventListener('scroll', handleResizeOrScroll, true);
});

onUnmounted(() => {
  document.removeEventListener('click', handleClickOutside);
  document.removeEventListener('keydown', handleKeydown);
  window.removeEventListener('resize', handleResizeOrScroll);
  window.removeEventListener('scroll', handleResizeOrScroll, true);
});
</script>

<template>
  <span ref="triggerWrapperRef" class="dropdown-trigger-wrapper" v-bind="attrs">
    <slot
      name="trigger"
      :is-open="isOpen"
      :trigger-props="{
        'aria-expanded': isOpen,
        'aria-haspopup': 'true',
        'aria-controls': dropdownId,
        onClick: toggleDropdown,
        onKeydown: handleTriggerKeydown
      }"
    />
  </span>

  <Teleport to="body">
    <ul
      v-if="isOpen && list.length"
      :id="dropdownId"
      ref="menuRef"
      class="dropdown-menu"
      :class="alignClass"
      :style="menuStyles"
      role="menu"
      @keydown="handleMenuKeydown"
    >
      <li v-for="(item, index) in list" :key="item.id || index" role="none">
        <button
          type="button"
          class="dropdown-item"
          role="menuitem"
          :disabled="item.disabled"
          @click="handleSelect(item)"
        >
          <component
            v-if="item.icon"
            :is="item.icon"
            :size="16"
            color="var(--color-accent)"
            aria-hidden="true"
          />
          <span>{{ item.label }}</span>
        </button>
      </li>
    </ul>
  </Teleport>
</template>

<style scoped>
.dropdown-trigger-wrapper {
  display: inline-block;
}

.dropdown-menu {
  background-color: var(--color-bg);
  border: 1px solid var(--color-border);
  border-radius: 0.75rem;
  padding: 0.25rem;
  list-style: none;
  margin: 0;
}

.dropdown-item {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  width: 100%;
  padding: 0.75rem 0.75rem;
  font-size: 1rem;
  border: none;
  background: none;
  cursor: pointer;
  border-radius: 0.5rem;
  transition: background-color 0.15s ease;
  outline: none;
}

.dropdown-item:focus-visible,
.dropdown-item:hover:not(:disabled) {
  background-color: var(--color-accent-hover);
}

.dropdown-item:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
