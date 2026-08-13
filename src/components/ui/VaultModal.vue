<script setup lang="ts">
import { ref, computed } from 'vue';
import { Vault } from '../../types/index.ts';
import Button from './Button.vue';
import AccountField from './AccountField.vue';
import ColorPicker from './ColorPicker.vue';
import useAppStore from '../../stores/appStore.ts';
import { VAULT_COLORS } from '../../util/constants.ts';
import { useRouter } from 'vue-router';

const props = defineProps<{
  vault?: Vault | null;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

const isEditMode = computed(() => !!props.vault);

const router = useRouter();
const { createNewVault, updateVaultDetails } = useAppStore();

const form = ref<{ name: string; color: string }>({
  name: props.vault?.name || '',
  color: props.vault?.color || VAULT_COLORS[0].hex
});

const isLoading = ref(false);

async function handleConfirm() {
  if (!form.value.name.trim() || isLoading.value) return;

  isLoading.value = true;

  const data = {
    name: form.value.name.trim(),
    color: form.value.color
  };

  try {
    if (isEditMode.value && props.vault) {
      await updateVaultDetails(props.vault.id, data);
    } else {
      const vault = await createNewVault(data);

      if (vault) {
        router.push({
          name: 'vault',
          params: {
            vaultId: vault.id,
            accountId: undefined
          }
        });
      }
    }

    emit('close');
  } finally {
    isLoading.value = false;
  }
}
</script>

<template>
  <form @submit.prevent="handleConfirm" :aria-busy="isLoading">
    <article class="container">
      <header>
        <h2>{{ isEditMode ? 'Edit Vault' : 'New Vault' }}</h2>
      </header>

      <main>
        <section class="vault-fields-section">
          <AccountField
            label="name"
            type="text"
            required
            input
            v-model="form.name"
            placeholder="Vault Name"
          />

          <div class="color-section">
            <ColorPicker v-model="form.color" :disabled="isLoading" />
          </div>
        </section>
      </main>

      <footer>
        <Button variant="outline" :disabled="isLoading" @click="emit('close')">Cancel</Button>
        <Button type="submit" :disabled="isLoading">
          {{ isLoading ? (isEditMode ? 'Saving…' : 'Creating…') : isEditMode ? 'Save' : 'Create' }}
        </Button>
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
  display: flex;
  flex-direction: column;
  gap: 1.5rem;
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

.vault-fields-section {
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

.color-section {
  display: flex;
  flex-direction: column;
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
</style>
