<script setup lang="ts">
import { useRoute } from 'vue-router';
import PasswordsList from '../panel/PasswordsList.vue';

const route = useRoute();
</script>

<template>
  <PasswordsList :vault_id="route.params.vaultId as string" />
</template>
