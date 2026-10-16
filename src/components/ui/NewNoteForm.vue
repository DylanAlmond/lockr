<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue';
import { Vault } from '../../types/index.ts';
import Button from './Button.vue';
import AccountField from './AccountField.vue';
import { CreateItemProps, useVault } from '../../composables/useVault.ts';
import Select from './Select.vue';
import IconUpload from './IconUpload.vue';

const { getUnlockedVaults } = useVault();

const props = defineProps<{ defaultVaultId?: string | null }>();

const vaults = ref<Vault[]>([]);
const manuallySetIcon = ref(false);

const form = ref<{
  vaultId: string;
  icon: string | null;
  title: string | null;
  body: string;
}>({
  vaultId: '',
  icon: null,
  title: null,
  body: ''
});

const displayInitial = computed(() => (form.value.title || 'Note')[0].toUpperCase());

const emit = defineEmits<{
  (e: 'submit', data: CreateItemProps): void;
  (e: 'close'): void;
}>();

function handleSubmit() {
  if (!form.value.vaultId || !form.value.body.trim()) return;

  emit('submit', {
    vaultId: form.value.vaultId,
    icon: form.value.icon,
    data: {
      kind: 'note',
      title: form.value.title || null,
      body: form.value.body
    }
  });
}

// Pre-fill the in-context vault once vaults are known
watch(
  () => [vaults.value, props.defaultVaultId],
  () => {
    if (!form.value.vaultId && props.defaultVaultId) {
      form.value.vaultId = props.defaultVaultId;
    }
  },
  { immediate: true }
);

onMounted(async () => {
  vaults.value = await getUnlockedVaults();
});
</script>

<template>
  <form @submit.prevent="handleSubmit">
    <article class="container">
      <header>
        <h2>New Note</h2>
      </header>

      <IconUpload
        v-model="form.icon"
        :fallback-text="displayInitial"
        aria-label="Click to upload note icon"
        hint="Click icon to upload image"
        @update:model-value="manuallySetIcon = true"
      />

      <main>
        <section class="note-fields-section">
          <div class="select-field">
            <h2 id="move-vault-label">vault</h2>
            <Select
              v-model="form.vaultId"
              :options="vaults.map((v) => ({ value: v.id, label: v.name }))"
              required
              fill
            />
          </div>

          <!-- Title -->
          <AccountField
            label="title"
            type="text"
            input
            v-model="form.title"
            placeholder="Title"
          />

          <!-- Body -->
          <div class="body-field">
            <div class="field-meta">
              <h2>body</h2>
              <textarea
                v-model="form.body"
                class="body-input"
                rows="10"
                placeholder="Start typing…"
                required
              ></textarea>
            </div>
          </div>
        </section>
      </main>

      <footer>
        <Button variant="outline" @click="emit('close')">Cancel</Button>
        <Button type="submit">Create</Button>
      </footer>
    </article>
  </form>
</template>

<style scoped>
.container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1.5rem;
  width: 100%;
}

header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-top: 0.5rem;
  width: 100%;
}

header > h2 {
  font-size: 1.5rem;
  font-family: var(--font-geo);
  text-align: center;
  width: 100%;
}

main {
  color: var(--color-text-secondary);
  line-height: 1.5rem;
  width: 100%;
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
  width: 100%;
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

.select-field {
  display: flex;
  flex-direction: column;
  padding: 1rem 1.5rem;
  overflow: hidden;
  border: 1px solid var(--color-border);
  transition:
    box-shadow 0.2s ease,
    border-color 0.2s ease;

  > h2 {
    font-weight: 400;
    font-size: 0.875rem;
    color: var(--color-accent-muted);
    margin-bottom: 0.25rem;
  }

  &:focus-within {
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  @supports (corner-shape: squircle) {
    corner-shape: squircle;
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
</style>
