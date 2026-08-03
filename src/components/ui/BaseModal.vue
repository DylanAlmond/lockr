<script setup lang="ts">
import { ref, watch } from 'vue';
import { useModal } from '../../composables/useModal';

const { isOpen, modalState, closeModal } = useModal();
const dialogRef = ref<HTMLDialogElement | null>(null);

watch(isOpen, (newValue) => {
  if (newValue) {
    dialogRef.value?.showModal();
  } else {
    if (dialogRef.value?.open) {
      dialogRef.value?.close();
    }
  }
});

function handleNativeClose() {
  closeModal();
}

function handleOutsideClick(event: MouseEvent) {
  if (event.target === dialogRef.value) {
    closeModal();
  }
}
</script>

<template>
  <dialog ref="dialogRef" class="modal" @click="handleOutsideClick" @close="handleNativeClose">
    <div v-if="modalState.component" class="modal-container">
      <component
        :is="modalState.component"
        v-bind="modalState.props"
        @close="closeModal"
        @confirm="closeModal"
      />
    </div>
  </dialog>
</template>

<style scoped>
.modal {
  margin: auto;
  border: none;
  box-sizing: border-box;
  border-radius: 12px;
  padding: 1.5rem;
  background: transparent;
  max-width: 400px;
  width: 90%;
  box-shadow: var(--shadow-sm);
  background-color: var(--color-bg);
}

.modal::backdrop {
  background-color: rgba(0, 0, 0, 0.6);
  backdrop-filter: blur(4px);
}

.modal-container {
  overflow: hidden;
}
</style>
