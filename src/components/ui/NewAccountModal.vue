<script setup lang="ts">
import { onMounted, ref, watch, computed } from 'vue';
import { Entropy, Vault } from '../../types/index.ts';
import Button from './Button.vue';
import AccountField from './AccountField.vue';
import { CreateAccountProps, useVault } from '../../composables/useVault.ts';
import { PASSWORDSTRENGTHS } from '../../util/constants.ts';
import { useRoute, useRouter } from 'vue-router';
import { Eye, EyeOff, Image } from '@lucide/vue';
import useAppStore from '../../stores/appStore.ts';
import {
  selectImageFile,
  processImageToBase64,
  fetchBrandLogoAsBase64
} from '../../util/imageUpload.ts';
import Select from './Select.vue';

const route = useRoute();
const router = useRouter();

const { getPasswordStrength, getUnlockedVaults } = useVault();
const { createNewAccount } = useAppStore();

const vaults = ref<Vault[]>([]);
const isUploadingIcon = ref(false);
const isFetchingLogo = ref(false);
const manuallySetIcon = ref(false);
let displayNameDebounceTimer: ReturnType<typeof setTimeout> | null = null;

const form = ref<CreateAccountProps>({
  vaultId: (route.params.vaultId as string) || '',
  username: '',
  password: ''
});

const passwordEntropy = ref<Entropy | null>(null);
const showPassword = ref(false);

// Compute the display icon - either the uploaded base64 image or the initials
const displayIcon = computed(() => {
  return form.value.icon;
});

const displayInitial = computed(() => {
  return (form.value.displayName || form.value.username || '')[0]?.toUpperCase() || '?';
});

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'confirm'): void;
}>();

// Auto-fetch logo when display name changes (if icon wasn't manually set)
watch(
  () => form.value.displayName,
  (displayName) => {
    if (!displayName || manuallySetIcon.value) {
      return;
    }

    // Debounce the logo fetch to avoid too many requests
    if (displayNameDebounceTimer) {
      clearTimeout(displayNameDebounceTimer);
    }

    displayNameDebounceTimer = setTimeout(async () => {
      try {
        isFetchingLogo.value = true;
        const logo = await fetchBrandLogoAsBase64(displayName);
        if (logo) {
          form.value.icon = logo;
        }
      } catch (error) {
        console.error('Logo fetch error:', error);
      } finally {
        isFetchingLogo.value = false;
      }
    }, 500);
  }
);

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
    manuallySetIcon.value = true; // Mark as manually set so we don't auto-fetch
  } catch (error) {
    console.error('Icon upload failed:', error);
  } finally {
    isUploadingIcon.value = false;
  }
}

async function handleConfirm() {
  if (!form.value) return;

  const account = await createNewAccount(form.value);

  if (account) {
    router.push({
      name: 'vault',
      params: {
        vaultId: account.vault_id,
        accountId: account.id
      }
    });
  }

  emit('close');
}

// Calculate password strength in real-time as the user types
watch(
  () => form.value.password,
  async (val) => {
    if (!val) {
      passwordEntropy.value = null;
      return;
    }
    passwordEntropy.value = await getPasswordStrength(val);
  },
  { immediate: true }
);

onMounted(async () => {
  vaults.value = await getUnlockedVaults();
});
</script>

<template>
  <form @submit.prevent="handleConfirm">
    <article class="container">
      <header>
        <h2>New Account</h2>
      </header>

      <section class="icon-section">
        <div
          class="account-icon"
          :class="{ 'cursor-pointer hover:opacity-80': !isUploadingIcon, loading: isUploadingIcon }"
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
        <p class="icon-hint">Click icon to upload image</p>
      </section>

      <main>
        <section class="account-fields-section">
          <div class="select-field">
            <h2 id="move-vault-label">vault</h2>
            <Select
              v-model="form.vaultId"
              :options="vaults.map((v) => ({ value: v.id, label: v.name }))"
              required
              fill
            />
          </div>

          <!-- Display Name -->
          <AccountField
            label="display name"
            type="text"
            input
            v-model="form.displayName"
            placeholder="Display Name"
          />

          <!-- Username -->
          <AccountField
            label="username"
            type="text"
            required
            input
            v-model="form.username"
            placeholder="Username"
          />

          <!-- Email -->
          <AccountField
            label="email"
            type="email"
            input
            v-model="form.email"
            placeholder="email@example.com"
          />

          <!-- Password -->
          <AccountField
            label="password"
            :type="showPassword ? 'text' : 'password'"
            input
            v-model="form.password"
            placeholder="Enter password"
          >
            <template #actions>
              <span v-if="passwordEntropy !== null" class="password-strength">
                {{ PASSWORDSTRENGTHS[passwordEntropy.score] }}
              </span>

              <Button
                :disabled="!form.password.length"
                aria-label="Toggle password visibility"
                icon-only
                variant="outline"
                size="small"
                :icon-component="showPassword ? EyeOff : Eye"
                @click="showPassword = !showPassword"
              />
            </template>
          </AccountField>
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

.password-strength {
  margin-right: 0.5rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-green);
  white-space: nowrap;
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

.icon-section {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 0.5rem;
  padding: 1rem 0;
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
