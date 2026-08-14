<script setup lang="ts">
import { onMounted, ref, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { Vault } from '../../types/index.ts';
import Button from './Button.vue';
import { useVault } from '../../composables/useVault.ts';
import useAppStore from '../../stores/appStore.ts';
import Select from './Select.vue';

const { getUnlockedVaults } = useVault();
const { state, moveActiveAccount } = useAppStore();
const route = useRoute();
const router = useRouter();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

const vaults = ref<Vault[]>([]);
const selectedVaultId = ref('');
const isLoading = ref(false);

const otherVaults = computed(() =>
  vaults.value.filter((v) => v.id !== state.activeAccount?.vault_id)
);

async function handleConfirm() {
  if (!selectedVaultId.value || isLoading.value) return;

  isLoading.value = true;
  try {
    const movedAccount = await moveActiveAccount(selectedVaultId.value);

    if (movedAccount) {
      await router.replace({
        name: route.name as string,
        params: {
          ...route.params,
          vaultId: movedAccount.vault_id,
          accountId: movedAccount.id,
          mode: undefined
        },
        query: route.query
      });
      emit('confirm');
      emit('close');
    }
  } finally {
    isLoading.value = false;
  }
}

onMounted(async () => {
  vaults.value = await getUnlockedVaults();
});
</script>

<template>
  <form @submit.prevent="handleConfirm" :aria-busy="isLoading">
    <article class="container">
      <header>
        <h2>Move to Vault</h2>
      </header>

      <main>
        <p class="description">
          Choose a vault to move
          {{
            state.activeAccount?.display_name
              ? `"${state.activeAccount.display_name}"`
              : 'this account'
          }}
          to.
        </p>

        <div class="select-section">
          <h2 id="move-vault-label">vault</h2>
          <Select
            v-model="selectedVaultId"
            required
            :options="vaults.map((v) => ({ value: v.id, label: v.name }))"
            :disabled="isLoading || !otherVaults.length"
            aria-labelledby="move-vault-label"
            fill
          />
        </div>

        <p v-if="!otherVaults.length" class="empty-hint">
          No other unlocked vaults available to move this account to.
        </p>
      </main>

      <footer>
        <Button variant="outline" :disabled="isLoading" @click="emit('close')">Cancel</Button>
        <Button type="submit" :disabled="isLoading || !selectedVaultId || !otherVaults.length">
          {{ isLoading ? 'Moving…' : 'Move' }}
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
  gap: 1rem;
  color: var(--color-text-secondary);
  line-height: 1.5rem;
  width: 100%;
}

.description {
  font-size: 0.9375rem;
  text-align: center;
}

.select-section {
  display: flex;
  flex-direction: column;
  padding: 1rem 1.5rem;
  overflow: hidden;
  border-radius: 0.75rem;
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

.empty-hint {
  font-size: 0.875rem;
  color: var(--color-text-tertiary);
  text-align: center;
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 0.75rem;
  width: 100%;
}
</style>
