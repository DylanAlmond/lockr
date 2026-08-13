<script setup lang="ts">
import { computed, markRaw, ref, watch } from 'vue';
import { useRouter } from 'vue-router';
import { ArrowLeft } from '@lucide/vue';
import AppShell from '../layout/AppShell.vue';
import Button from '../ui/Button.vue';
import AccountField from '../ui/AccountField.vue';
import IconUpload from '../ui/IconUpload.vue';
import ColorPicker from '../ui/ColorPicker.vue';
import AlertModal from '../ui/AlertModal.vue';
import { useModal } from '../../composables/useModal.ts';
import { useUser } from '../../composables/useUser.ts';
import { VAULT_COLORS } from '../../util/constants.ts';

const router = useRouter();
const { user, isLoading, updateProfile, deleteUser } = useUser();
const { openModal } = useModal();

const form = ref<{ name: string; color: string; icon: string | null }>({
  name: '',
  color: VAULT_COLORS[0].hex,
  icon: null
});

const displayInitial = computed(() => (form.value.name || 'U')[0]?.toUpperCase() || '?');

const hasChanges = computed(() => {
  if (!user.value) return false;
  return (
    form.value.name !== user.value.name ||
    form.value.color !== user.value.color ||
    form.value.icon !== user.value.icon
  );
});

async function handleSave() {
  if (!hasChanges.value || !form.value.name.trim()) return;
  await updateProfile({
    name: form.value.name.trim(),
    color: form.value.color,
    icon: form.value.icon
  });
}

function handleDeleteAccount() {
  openModal(markRaw(AlertModal), {
    title: 'Delete Account',
    message:
      "Are you sure you want to continue? This will permanently delete your account profile from this device. This can't be undone.",
    actionLabel: 'Delete',
    confirmationValue: user.value?.name,
    onClose: () => {},
    onConfirm: async () => {
      const success = await deleteUser();

      if (success) {
        router.replace('/auth');
      }
    }
  });
}

// Keep the local form in sync whenever the backend user profile changes
watch(
  user,
  (current) => {
    form.value = {
      name: current?.name || '',
      color: current?.color || VAULT_COLORS[0].hex,
      icon: current?.icon || null
    };
  },
  { immediate: true }
);
</script>

<template>
  <AppShell>
    <template #titlebar>
      <Button
        class="back-button"
        aria-label="Back"
        variant="label"
        icon-only
        :icon-component="ArrowLeft"
        @click="router.back()"
      />
      <!-- <h1 class="titlebar-heading">Settings</h1> -->
    </template>

    <div class="settings-page thin-scrollbar">
      <div class="settings-container">
        <section class="settings-section">
          <h2>Profile</h2>

          <form class="profile-form" @submit.prevent="handleSave">
            <div class="profile-row">
              <IconUpload
                v-model="form.icon"
                :fallback-text="displayInitial"
                aria-label="Click to upload profile picture"
              />

              <AccountField
                class="name-field"
                label="name"
                type="text"
                required
                input
                v-model="form.name"
                placeholder="Your name"
              />
            </div>

            <ColorPicker v-model="form.color" :disabled="isLoading" />

            <div class="section-footer">
              <Button type="submit" :disabled="isLoading || !hasChanges || !form.name.trim()">
                {{ isLoading ? 'Saving…' : 'Save Changes' }}
              </Button>
            </div>
          </form>
        </section>

        <section class="settings-section">
          <h2>Vault Data</h2>
          <p id="vault-data-hint" class="section-hint">
            Import or export your vaults. Coming soon.
          </p>

          <div class="section-actions">
            <Button
              variant="outline"
              disabled
              title="Coming soon"
              aria-describedby="vault-data-hint"
            >
              Import Vaults
            </Button>
            <Button
              variant="outline"
              disabled
              title="Coming soon"
              aria-describedby="vault-data-hint"
            >
              Export Vaults
            </Button>
          </div>
        </section>

        <section class="settings-section danger-zone">
          <h2>Danger Zone</h2>
          <p class="section-hint">
            Permanently delete your account from this device. This can't be undone.
          </p>

          <div class="section-actions">
            <Button variant="danger" @click="handleDeleteAccount"> Delete Account </Button>
          </div>
        </section>
      </div>
    </div>
  </AppShell>
</template>

<style scoped>
.titlebar-heading {
  font-family: var(--font-geo);
  font-size: 1.25rem;
  font-weight: 500;
  margin: auto 0;
  margin-left: auto;
}

.back-button {
  margin-right: auto;
}

.settings-page {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
}

.settings-container {
  display: flex;
  flex-direction: column;
  gap: 3rem;
  max-width: 32rem;
  margin: 0 auto;
  padding: 2.5rem 1.5rem;
}

.settings-section {
  display: flex;
  flex-direction: column;
  gap: 1rem;

  > h2 {
    font-family: var(--font-geo);
    font-size: 1.25rem;
    font-weight: 500;
  }
}

.section-hint {
  margin-top: -0.5rem;
  color: var(--color-text-muted);
  font-size: 0.875rem;
}

.section-actions {
  display: flex;
  gap: 0.75rem;
}

.section-footer {
  display: flex;
  /* justify-content: flex-end; */
}

.profile-form {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.profile-row {
  display: flex;
  align-items: center;
  gap: 1.25rem;

  > :last-child {
    flex: 1;
  }
}

.name-field {
  width: 100%;
  border-radius: 0.75rem;
  flex: 1;
}

.danger-zone {
  padding-top: 1rem;
  border-top: 1px solid var(--color-border);

  > h2 {
    color: var(--color-red);
  }
}
</style>
