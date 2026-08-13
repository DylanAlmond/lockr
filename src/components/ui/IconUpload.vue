<script setup lang="ts">
import { ref } from 'vue';
import { Image } from '@lucide/vue';
import { selectImageFile, processImageToBase64 } from '../../util/imageUpload.ts';

interface Props {
  modelValue?: string | null;
  fallbackText: string;
  ariaLabel?: string;
  hint?: string;
}

withDefaults(defineProps<Props>(), {
  modelValue: null,
  ariaLabel: 'Click to upload image'
});

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void;
}>();

const isUploading = ref(false);

async function handleUpload() {
  if (isUploading.value) return;

  try {
    isUploading.value = true;
    const file = await selectImageFile();

    if (!file) {
      return;
    }

    const base64 = await processImageToBase64(file);
    emit('update:modelValue', base64);
  } catch (error) {
    console.error('Icon upload failed:', error);
  } finally {
    isUploading.value = false;
  }
}
</script>

<template>
  <div class="icon-upload">
    <div
      class="account-icon"
      :class="{ loading: isUploading }"
      @click="handleUpload"
      role="button"
      tabindex="0"
      :aria-label="ariaLabel"
      :aria-disabled="isUploading"
      @keydown.enter="handleUpload"
      @keydown.space.prevent="handleUpload"
    >
      <img v-if="modelValue" :src="modelValue" alt="" class="icon-image" />
      <span v-else class="icon-text">{{ fallbackText }}</span>
      <Image class="upload-overlay" :size="28" />
    </div>
    <p v-if="hint" class="icon-hint">{{ hint }}</p>
  </div>
</template>

<style scoped>
.icon-upload {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
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
  cursor: pointer;
  transition: all 0.2s ease;

  &:hover:not(.loading) {
    opacity: 0.6;
    color: transparent;

    .upload-overlay {
      opacity: 1;
    }
  }

  &.loading {
    cursor: not-allowed;
    opacity: 0.6;
  }

  &:focus-visible {
    outline: none;
    box-shadow: inset 0 0 0 2px var(--color-accent);
  }

  .icon-image {
    width: 100%;
    height: 100%;
    object-fit: cover;
    border-radius: 0.75rem;
  }

  .icon-text {
    pointer-events: none;
  }

  .upload-overlay {
    position: absolute;
    opacity: 0;
    transition: opacity 0.2s ease;
    color: var(--color-accent);
    pointer-events: none;
  }
}

.icon-hint {
  font-size: 0.75rem;
  color: var(--color-text-muted);
  margin: 0;
}
</style>
