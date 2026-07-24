<script setup lang="ts">
import { useRoute } from 'vue-router';
import { useVault } from '../../composables/useVault';
import { Account } from '../../types';
import { ref, watch } from 'vue';

const route = useRoute();
const { getAccountbyId } = useVault();

const account = ref<Account | null>(null);
const loading = ref(false);

watch(
  () => route.params.passwordId,
  async (id) => {
    if (!id) {
      account.value = null;
      return;
    }

    loading.value = true;

    try {
      account.value = await getAccountbyId(id as string);
    } finally {
      loading.value = false;
    }
  },
  { immediate: true }
);
</script>

<template>
  <h2>{{ account?.display_name || account?.username }}</h2>
</template>
