<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { Account } from '../../types';
import { ChevronRight, Image } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import Input from '../ui/Input.vue';
import useAppStore from '../../stores/appStore.ts';
import { selectImageFile, processImageToBase64 } from '../../util/imageUpload.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveAccount } = useAppStore();

const form = ref<Partial<Account>>({});
const isUploadingIcon = ref(false);
// const isFetchingLogo = ref(false);
const manuallySetIcon = ref(false);
// let displayNameDebounceTimer: ReturnType<typeof setTimeout> | null = null;

// Compute the display icon - either the uploaded base64 image or the initials
const displayIcon = computed(() => {
  return form.value.icon;
});

const displayInitial = computed(() => {
  return (form.value.display_name || form.value.username || '')[0]?.toUpperCase() || '?';
});

// Sync local form state whenever the globally active account changes
watch(
  () => state.activeAccount,
  (newAccount) => {
    form.value = newAccount ? { ...newAccount } : {};
    manuallySetIcon.value = false; // Reset when loading a new account
  },
  { immediate: true }
);

// Auto-fetch logo when display name changes (if icon wasn't manually set)
// watch(
//   () => form.value.display_name,
//   (displayName) => {
//     if (!displayName || manuallySetIcon.value) {
//       return;
//     }

//     // Debounce the logo fetch to avoid too many requests
//     if (displayNameDebounceTimer) {
//       clearTimeout(displayNameDebounceTimer);
//     }

//     displayNameDebounceTimer = setTimeout(async () => {
//       try {
//         isFetchingLogo.value = true;
//         const logo = await fetchBrandLogoAsBase64(displayName);
//         if (logo) {
//           form.value.icon = logo;
//         }
//       } catch (error) {
//         console.error('Logo fetch error:', error);
//       } finally {
//         isFetchingLogo.value = false;
//       }
//     }, 500);
//   }
// );

function handleCancel() {
  router.push({
    name: route.name as string,
    params: { ...route.params, mode: undefined },
    query: route.query
  });
}

async function handleIconUpload() {
  try {
    isUploadingIcon.value = true;
    const file = await selectImageFile();

    if (!file) {
      isUploadingIcon.value = false;
      return;
    }

    const base64 = await processImageToBase64(file);
    form.value.icon = base64;
  } catch (error) {
    console.error('Icon upload failed:', error);
  } finally {
    isUploadingIcon.value = false;
  }
}

async function handleSave() {
  if (!state.activeAccount) return;

  const updated = await updateActiveAccount(form.value);

  if (updated) {
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
  <div v-if="!state.activeAccount" class="wrapper">No account found.</div>

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
        <div 
          class="account-icon"
          :class="{ 'cursor-pointer hover:opacity-80': !isUploadingIcon, 'loading': isUploadingIcon }"
          @click="handleIconUpload"
          role="button"
          tabindex="0"
          aria-label="Click to upload account icon"
          @keydown.enter="handleIconUpload"
          @keydown.space="handleIconUpload"
        >
          <img v-if="displayIcon" :src="displayIcon" alt="Account icon" class="icon-image" />
          <span v-else class="icon-text">{{ displayInitial }}</span>
          <Image class="upload-overlay" :size="28" />
        </div>

        <Input
          name="display name"icon-image
          class="display-name"
          v-model="form.display_name"
          placeholder="Display Name"
        />
      </section>

      <section class="account-fields-section">
        <!-- Username -->
        <AccountField
          label="username"
          type="text"
          input
          v-model="form.username"
          placeholder="No Value"
        />

        <!-- Email -->
        <AccountField
          label="email"
          type="email"
          input
          v-model="form.email"
          placeholder="No Value"
        />
      </section>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList v-model="form.tags" editable />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(state.activeAccount.updated_at)} ` }}</span>
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

.display-name {
  flex: 1;
}

.account-fields-section {
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
