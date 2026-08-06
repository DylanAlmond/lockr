import { ref, shallowRef, type Component } from 'vue';

export interface ModalState {
  component: Component | null;
  props: Record<string, unknown>;
}

type ComponentProps<T> = T extends new (...args: any[]) => any
  ? InstanceType<T>['$props']
  : Record<string, unknown>;

// Global singleton state
const isOpen = ref<boolean>(false);
const modalState = shallowRef<ModalState>({
  component: null,
  props: {}
});

export function useModal() {
  function openModal<T extends Component>(component: T, props: ComponentProps<T>): void {
    modalState.value = {
      component,
      props: props as Record<string, unknown>
    };
    isOpen.value = true;
  }

  function closeModal() {
    isOpen.value = false;
    modalState.value = { component: null, props: {} };
  }

  return {
    isOpen,
    modalState,
    openModal,
    closeModal
  };
}
