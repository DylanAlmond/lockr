<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, nextTick, useAttrs } from 'vue';
import type { Component, CSSProperties } from 'vue';

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

const attrs = useAttrs();

const emit = defineEmits<{
  (e: 'select', item: DropdownItem): void;
}>();

const isOpen = ref(false);
const triggerWrapperRef = ref<HTMLElement | null>(null);
const menuRef = ref<HTMLUListElement | null>(null);
const menuStyles = ref<CSSProperties>({});

const dropdownId = `dropdown-menu-${Math.random().toString(36).substring(2, 9)}`;
const alignClass = computed(() => `dropdown-menu--${props.align}`);

const updatePosition = () => {
  const trigger = triggerWrapperRef.value;
  const menu = menuRef.value;

  if (!trigger || !menu) return;

  const rect = trigger.getBoundingClientRect();

  const menuWidth = menu.offsetWidth || rect.width;
  const menuHeight = menu.offsetHeight || 200;

  const viewportWidth = window.innerWidth;
  const viewportHeight = window.innerHeight;

  // 1. Calculate Left Position
  let leftPos = rect.left;
  if (props.align === 'right') {
    leftPos = rect.right - menuWidth;
  }

  // Clamp left position to stay within viewport (with 8px padding)
  const maxLeft = viewportWidth - menuWidth - 8;
  leftPos = Math.max(8, Math.min(leftPos, maxLeft));

  // 2. Calculate Top Position
  const spaceBelow = viewportHeight - rect.bottom;
  const spaceAbove = rect.top;
  let topPos: number;

  // Decide whether to open downwards or upwards
  if (spaceBelow >= menuHeight + 8 || spaceBelow >= spaceAbove) {
    topPos = rect.bottom + 8; // Open downwards
  } else {
    topPos = rect.top - menuHeight - 8; // Open upwards
  }

  // Final clamping for top to prevent off-screen issues on very short screens
  const maxTop = viewportHeight - menuHeight - 8;
  topPos = Math.max(8, Math.min(topPos, maxTop));

  menuStyles.value = {
    position: 'fixed',
    top: `${topPos}px`,
    left: `${leftPos}px`,
    minWidth: `${rect.width}px`,
    zIndex: '9999'
  };
};

const toggleDropdown = async (event: Event) => {
  event.stopPropagation();
  isOpen.value = !isOpen.value;

  if (isOpen.value) {
    // Wait for the DOM to render the <ul> element before measuring it
    await nextTick();
    updatePosition();

    // Focus if not mouse toggled (detail === 0 means keyboard interaction)
    if ((event as MouseEvent).detail === 0) {
      focusFirstItem();
    }
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
  items?.[0]?.focus();
};

const handleSelect = (item: DropdownItem) => {
  if (item.disabled) return;
  if (item.onSelect) item.onSelect();
  emit('select', item);
  closeDropdown();
  focusTrigger();
};

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
      event.preventDefault();
      if (event.shiftKey) {
        if (currentIndex <= 0) {
          closeDropdown();
          focusTrigger();
        } else {
          items[currentIndex - 1].focus();
        }
      } else {
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
          <component v-if="item.icon" :is="item.icon" :size="16" aria-hidden="true" />
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
  padding: 0.375rem;
  list-style: none;
  margin: 0;
  box-sizing: border-box;

  width: 15rem;
  min-width: fit-content;
  max-width: calc(100vw - 1rem);
  max-height: calc(100vh - 2rem);
  overflow-y: auto;
}

.dropdown-item {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  width: 100%;
  height: 2.25rem;
  padding: 0.75rem 0.5rem;
  font-size: 0.875rem;
  border: none;
  background: none;
  cursor: pointer;
  border-radius: 0.5rem;
  transition: background-color 0.15s ease;
  outline: none;

  > svg {
    color: var(--color-accent);
  }
}

.dropdown-item:focus-visible,
.dropdown-item:hover:not(:disabled) {
  color: var(--color-bg);
  background-color: var(--color-accent);

  > svg {
    color: var(--color-bg);
  }
}

.dropdown-item:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
</style>
