<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted, nextTick } from 'vue';
import { useModal } from '../../composables/useModal';

const { isOpen, modalState, closeModal } = useModal();

const modalRef = ref<HTMLDivElement | null>(null);

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape' && isOpen.value) {
    closeModal();
  }
}

function handleBackdropClick(event: MouseEvent) {
  if (event.target === event.currentTarget) {
    closeModal();
  }
}

watch(isOpen, (open) => {
  if (open) {
    document.body.style.overflow = 'hidden';
    nextTick(() => modalRef.value?.focus());
  } else {
    document.body.style.overflow = '';
  }
});

onMounted(() => {
  document.addEventListener('keydown', handleKeydown);
});

onUnmounted(() => {
  document.removeEventListener('keydown', handleKeydown);
  document.body.style.overflow = '';
});
</script>

<template>
  <Teleport to="body">
    <div
      v-if="isOpen"
      ref="modalRef"
      class="modal-backdrop"
      tabindex="-1"
      @mousedown="handleBackdropClick"
    >
      <div class="modal thin-scrollbar" role="dialog" aria-modal="true" @mousedown.stop>
        <div v-if="modalState.component" class="modal-container">
          <component
            :is="modalState.component"
            v-bind="modalState.props"
            @close="closeModal"
            @confirm="closeModal"
          />
        </div>
      </div>
    </div>
  </Teleport>
</template>

<style scoped>
.modal-backdrop {
  position: fixed;
  inset: 0;
  z-index: 1000;
  pointer-events: none;

  display: flex;
  align-items: center;
  justify-content: center;

  background-color: rgba(0, 0, 0, 0.1);
  /* backdrop-filter: blur(4px); */
}

.modal {
  pointer-events: initial;
  box-sizing: border-box;
  width: 90%;
  max-width: 480px;
  max-height: calc(100% - 2rem);

  border-radius: 0.75rem;
  padding: 1.5rem;
  margin: 1rem;

  background-color: var(--color-bg);
  box-shadow: var(--shadow-sm);
  border: 1px solid var(--color-border);
  overflow-y: auto;

  outline: none;

  /* Corner smoothing */
  @supports (corner-shape: squircle) {
    corner-shape: squircle;
    border-radius: 1.5rem;
  }
}

.modal-container {
  overflow: hidden;
}
</style>
