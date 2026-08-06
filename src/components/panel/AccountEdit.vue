<script setup lang="ts">
import { Account } from '../../types';
import { ChevronRight } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import Input from '../ui/Input.vue';
import { ref, watch } from 'vue';
import useAppStore from '../../stores/appStore.ts';

const { state, setEditMode, updateActiveAccount } = useAppStore();

const form = ref<Partial<Account>>({});

// Sync local form state whenever the globally active account changes
watch(
  () => state.activeAccount,
  (newAccount) => {
    form.value = newAccount ? { ...newAccount } : {};
  },
  { immediate: true }
);

async function handleSave() {
  if (!state.activeAccount) return;

  const updated = await updateActiveAccount(form.value);

  if (updated) {
    setEditMode(false);
  }
}
</script>

<template>
  <!-- Empty -->
  <div v-if="!state.activeAccount" class="wrapper">No account found.</div>

  <!-- Edit Mode -->
  <div v-else class="wrapper">
    <header>
      <span class="editmode-label">Editing</span>

      <nav class="header-toolbar">
        <Button variant="solid" size="small" @click="setEditMode(false)">Cancel</Button>
        <Button size="small" @click="handleSave">Save</Button>
      </nav>
    </header>

    <main class="thin-scrollbar">
      <section class="descriptor-section">
        <div class="account-icon">
          {{ (state.activeAccount.display_name || state.activeAccount.username)[0].toUpperCase() }}
        </div>

        <Input
          name="display name"
          class="display-name"
          v-model="form.display_name"
          placeholder="Display Name"
        />
      </section>

      <section class="account-fields-section">
        <!-- Username -->
        <AccountField label="username" type="text" input v-model="form.username" />

        <!-- Email -->
        <AccountField label="email" type="email" input v-model="form.email" />
      </section>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList v-model="form.tags" editable />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(state.activeAccount.updated_at)} ` }}</span>
      </section>
    </main>
  </div>
</template>

<style scoped>
.wrapper {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
}
header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 1rem;
  background-color: #efecf9;
}
main {
  display: flex;
  flex-direction: column;
  box-sizing: border-box;
  gap: 1.5rem;
  padding: 1rem;
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
}
.header-toolbar {
  display: flex;
  gap: 0.75rem;
}
.descriptor-section {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 0.75rem 0rem;
}
.account-icon {
  position: relative;
  display: flex;
  justify-content: center;
  align-items: center;
  width: 5.25rem;
  height: 5.25rem;
  aspect-ratio: 1/1;
  font-size: 2rem;
  font-family: var(--font-geo);
  font-weight: 500;
  background-color: var(--color-accent-hover);
  color: var(--color-accent);
  border-radius: 0.75rem;
  box-shadow: var(--shadow-sm);
}
.display-name {
  flex: 1;
}
.account-fields-section {
  display: flex;
  flex-direction: column;
  width: 100%;
  & > *:first-child {
    border-radius: 0.75rem 0.75rem 0 0;
  }
  & > *:last-child {
    border-radius: 0 0 0.75rem 0.75rem;
  }
  & > *:not(:last-child) {
    border-bottom: none;
  }
}
.tags-section {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  padding: 0rem 1.5rem;
  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }
}
.timestamp-section {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  padding: 1.5rem;
  padding-right: 0rem;
  text-box-trim: trim-both;
  text-box-edge: cap alphabetic;
  > svg {
    color: var(--color-text-muted);
  }
}
.editmode-label {
  font-family: var(--font-ui);
  font-size: 1.25rem;
  font-weight: 500;
  color: var(--color-accent-muted);
}
</style>
