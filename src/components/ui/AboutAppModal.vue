<script setup lang="ts">
import Button from './Button.vue';
import Logo from '../../assets/logo.svg';
import { getVersion } from '@tauri-apps/api/app';
import { onMounted, ref } from 'vue';

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

const appVersion = ref('');
const currentYear = new Date().getFullYear();

onMounted(async () => {
  appVersion.value = await getVersion();
});
</script>

<template>
  <article class="container">
    <header>
      <Logo class="logo" />
      <span class="version">Version Beta {{ appVersion }}</span>
    </header>

    <main>
      <p>
        A desktop application built with
        <strong>Tauri</strong>, <strong>Vue</strong>, and <strong>TypeScript</strong>.
      </p>

      <p class="copyright">© {{ currentYear }} Dylan Almond</p>
    </main>

    <footer>
      <Button @click="emit('close')">Close</Button>
    </footer>
  </article>
</template>

<style scoped>
.container {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 1.5rem;
}

header {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
  margin-top: 0.5rem;
}

.logo {
  height: 5rem;
}

main {
  display: flex;
  flex-direction: column;
  gap: 1rem;
  text-align: center;
  color: var(--color-text-secondary);
  line-height: 2rem;
}

.version,
.copyright {
  font-size: 0.875rem;
  color: var(--color-text-secondary);
}

footer {
  display: flex;
  justify-content: flex-end;
  width: 100%;
  gap: 0.75rem;
}
</style>
