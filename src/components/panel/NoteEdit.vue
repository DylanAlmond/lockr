<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import IconUpload from '../ui/IconUpload.vue';
import useAppStore from '../../stores/appStore.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem } = useAppStore();

const note = computed(() =>
  state.activeItem?.kind === 'note' ? state.activeItem : null
);

const displayLabel = computed(() => note.value?.display_name || note.value?.title || 'Untitled');

const displayInitial = computed(() => displayLabel.value[0].toUpperCase());

const form = ref<{
  icon: string | null;
  title: string | null;
  body: string;
  tags: string[];
}>({
  icon: null,
  title: null,
  body: '',
  tags: []
});

// Sync local form state whenever the globally active note changes
watch(
  () => note.value,
  (newNote) => {
    if (newNote) {
      form.value = {
        icon: newNote.icon,
        title: newNote.title,
        body: newNote.body,
        tags: [...newNote.tags]
      };
    }
  },
  { immediate: true }
);

function handleCancel() {
  router.push({
    name: route.name as string,
    params: { ...route.params, mode: undefined },
    query: route.query
  });
}

async function handleSave() {
  if (!note.value) return;

  const success = await updateActiveItem({
    icon: form.value.icon,
    tags: form.value.tags,
    data: {
      kind: 'note',
      title: form.value.title,
      body: form.value.body
    }
  });

  if (success) {
    router.push({
      name: route.name as string,
      params: { ...route.params, mode: undefined },
      query: route.query
    });
  }
}
</script>

<template>
  <!-- Empty -->
  <div v-if="!note" class="wrapper">No note found.</div>

  <!-- Edit Mode -->
  <div v-else class="wrapper">
    <header>
      <span class="editmode-label">Editing</span>

      <nav class="header-toolbar">
        <Button variant="solid" size="small" @click="handleCancel">Cancel</Button>
        <Button size="small" @click="handleSave">Save</Button>
      </nav>
    </header>

    <main class="thin-scrollbar">
      <section class="descriptor-section">
        <IconUpload
          v-model="form.icon"
          :fallback-text="displayInitial"
          aria-label="Click to upload note icon"
        />
      </section>

      <section class="note-fields-section">
        <!-- Title -->
        <AccountField
          label="title"
          type="text"
          input
          v-model="form.title"
          placeholder="No Value"
        />

        <!-- Body -->
        <div class="body-field">
          <div class="field-meta">
            <h2>body</h2>
            <textarea
              v-model="form.body"
              class="body-input"
              rows="10"
              placeholder="No Value"
            ></textarea>
          </div>
        </div>
      </section>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList v-model="form.tags" editable />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(note.updated_at)} ` }}</span>
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
  flex: 1;
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

.note-fields-section {
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

.body-field {
  display: flex;
  align-items: center;
  gap: 1rem;
  padding: 1rem 1.5rem;
  overflow: hidden;
  border: 1px solid var(--color-border);
  transition:
    box-shadow 0.2s ease,
    border-color 0.2s ease;

  &:focus-within {
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
  }
}

.field-meta {
  flex: 1;
  min-width: 0;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }
}

.body-input {
  display: block;
  width: 100%;
  font: inherit;
  color: var(--color-text);
  background: transparent;
  border: none;
  outline: none;
  padding: 0;
  margin: 0;
  resize: vertical;
  min-height: 10rem;
  white-space: pre-wrap;
}

.body-input::placeholder {
  color: var(--color-text-muted);
  opacity: 1;
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
