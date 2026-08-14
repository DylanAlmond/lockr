<script setup lang="ts">
import { useRoute } from 'vue-router';
import { computed, watch } from 'vue';
import AccountDetails from './AccountDetails.vue';
import AccountEdit from './AccountEdit.vue';
import useAppStore from '../../stores/appStore.ts';

const route = useRoute();
const { setActiveAccount } = useAppStore();

const accountId = computed(() => route.params.accountId as string | undefined);
const mode = computed(() => route.params.mode as string | undefined);

const isCreate = computed(() => accountId.value === 'create' || mode.value === 'create');
const isEdit = computed(() => mode.value === 'edit' && !isCreate.value);

watch(
  accountId,
  (id) => {
    // Only fetch the account if we are viewing or editing an existing one
    if (id && !isCreate.value) {
      setActiveAccount(id);
    } else {
      // Clear active account if we are in "create" mode or no account is selected
      setActiveAccount(null);
    }
  },
  { immediate: true }
);
</script>

<template>
  <!-- 
    AccountEdit handles both Create and Edit natively. 
    It can check `isCreate` or if `state.activeAccount` is null to know it's creating a new item.
  -->
  <AccountEdit v-if="isCreate || isEdit" />

  <!-- Details view -->
  <AccountDetails v-else-if="accountId" />

  <!-- Empty state when no account is clicked -->
  <div v-else class="empty-panel">
    <p>Select an account to view details</p>
  </div>
</template>

<style scoped>
.empty-panel {
  display: flex;
  justify-content: center;
  align-items: center;
  height: 100%;
  color: var(--color-text-tertiary);
  flex: 1;
}
</style>
