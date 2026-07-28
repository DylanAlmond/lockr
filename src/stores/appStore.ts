import { reactive } from 'vue';

interface AppStore {
  editMode: boolean;
}

const state = reactive<AppStore>({
  editMode: false
});

function useAppStore() {
  function setEditMode(value: boolean) {
    state.editMode = value;
  }

  return {
    state,
    setEditMode
  };
}

export default useAppStore;
