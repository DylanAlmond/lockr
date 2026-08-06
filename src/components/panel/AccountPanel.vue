<script setup lang="ts">
import { useRoute } from 'vue-router';
import useAppStore from '../../stores/appStore.ts';
import { watch } from 'vue';
import AccountDetails from './AccountDetails.vue';
import AccountEdit from './AccountEdit.vue';

const route = useRoute();
const { state, setActiveAccount } = useAppStore();

watch(
  () => route.params.passwordId,
  (id) => {
    setActiveAccount((id as string) || null);
  },
  { immediate: true }
);
</script>

<template>
  <AccountEdit v-if="state.editMode" />
  <AccountDetails v-else />
</template>
