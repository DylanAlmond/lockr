<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { Account, Entropy } from '../../types';
import { ChevronRight } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import ItemField from '../ui/ItemField.vue';
import IconUpload from '../ui/IconUpload.vue';
import useAppStore from '../../stores/appStore.ts';
import { PASSWORDSTRENGTHS } from '../../util/constants.ts';
import { ACCOUNT_PASSWORD_MASK, SECRET_FIELD_PASSWORD } from '../../util/secrets.ts';
import { useVault } from '../../composables/useVault.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem, setActiveItem } = useAppStore();
const { getItemSecret, getPasswordStrength, updateItemSecret } = useVault();

// Narrowed on the account kind, like the other per-kind editors
const account = computed(() => (state.activeItem?.kind === 'account' ? state.activeItem : null));

const form = ref<Partial<Account>>({});
// const isFetchingLogo = ref(false);
const manuallySetIcon = ref(false);
// let displayNameDebounceTimer: ReturnType<typeof setTimeout> | null = null;

// The stored value is prefetched on mount; the input holds the real
// value, masked as password dots until the field is focused. A present
// value masks as the account password shape; an empty one shows
// 'No Value' like any other empty field.
const passwordInput = ref<string | null>(null);
const passwordOriginal = ref<string | null>(null);
const passwordEntropy = ref<Entropy | null>(null);

/** Set when the password fetch failed: the stored value is then unknown
 * and save must not push a password diff against it (a push could erase a
 * value we were never able to read). */
const passwordFetchFailed = ref(false);

// Like the other secret fields: revealed only while the field is focused
const passwordFocused = ref(false);

/** In-flight prefetch promise; awaited by handleSave so a fast save
 * cannot silently skip the password push while original is still null. */
let passwordFetch: Promise<void> | null = null;

const passwordPlaceholder = computed(() => {
  // Once the stored state is known: an empty-or-emptied field (the user
  // is clearing it) shows 'No Value'; a present value shows the mask.
  const fetched = passwordOriginal.value !== null;
  if (fetched && (passwordInput.value === '' || passwordOriginal.value === '')) {
    return 'No Value';
  }
  return ACCOUNT_PASSWORD_MASK;
});

async function fetchPassword() {
  const current = account.value;
  if (!current || passwordOriginal.value !== null) return;

  try {
    const stored = await getItemSecret(current.vault_id, current.id, SECRET_FIELD_PASSWORD);

    // The fetch may outlive this item's context; discard if it changed
    if (state.activeItem?.id !== current.id) return;

    passwordOriginal.value = stored ?? '';
    // Stamp the input only if the user hasn't typed while the fetch was
    // in flight: typed values win over the (just-learned) stored value,
    // and the save diff then holds against the now-known original
    // (assignment also triggers the strength watcher below)
    if (!passwordInput.value) {
      passwordInput.value = stored ?? '';
    }
  } catch {
    passwordFetchFailed.value = true;
  }
}

// Calculate password strength in real-time as the user types
watch(passwordInput, async (val) => {
  if (!val) {
    passwordEntropy.value = null;
    return;
  }
  passwordEntropy.value = await getPasswordStrength(val);
});

const displayInitial = computed(() => {
  return (form.value.display_name || form.value.username || '')[0]?.toUpperCase() || '?';
});

// Sync local form state whenever the globally active item changes
watch(
  () => state.activeItem,
  (item) => {
    const view = item?.kind === 'account' ? item : null;
    form.value = view ? { ...view } : {};
    manuallySetIcon.value = false; // Reset when loading a new item

    // Reset the password row and fetch for the (new) account
    passwordOriginal.value = null;
    passwordInput.value = null;
    passwordEntropy.value = null;
    passwordFocused.value = false;
    passwordFetchFailed.value = false;
    passwordFetch = fetchPassword();
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

async function handleSave() {
  if (!account.value) return;

  // The prefetch may still be in flight; wait for it so passwordOriginal
  // is known — otherwise the push guard below would silently skip the
  // password push because the fetched original was never learned.
  if (passwordFetch) {
    await passwordFetch;
  }

  // Snapshot the password FIRST: pushing metadata replaces the active
  // item object, which re-triggers the reset watcher below and would
  // otherwise wipe the fetched value before the password push runs.
  const original = passwordOriginal.value;
  const input = passwordInput.value;
  const item = account.value;

  const success = await updateActiveItem({
    displayName: form.value.display_name ?? null,
    icon: form.value.icon || null,
    favourite: form.value.favourite ?? null,
    tags: form.value.tags || [],
    color: form.value.color ?? null,
    data: {
      kind: 'account',
      username: form.value.username ?? '',
      email: form.value.email ?? null
    }
  });
  if (!success) return;

  // Push the password only when it changed from the fetched original.
  // Emptying the field removes the value, like any other secret field.
  // The snapshot's original is non-null only when the fetch actually
  // landed: a failed fetch stays null, so the push — which could erase a
  // stored password we couldn't read — is skipped (surfaced by the
  // fetch-failure notice next to the field).
  if (original !== null && input !== original) {
    await updateItemSecret(
      item.vault_id,
      item.id,
      SECRET_FIELD_PASSWORD,
      input || null
    );

    // Secret changes re-derive nothing on the view itself, but the
    // password row was reset by the metadata push — refresh the active
    // item so the details view shows the final state
    await setActiveItem(item.id);
  }

  router.push({
    name: route.name as string,
    params: { ...route.params, mode: undefined },
    query: route.query
  });
}
</script>

<template>
  <!-- Empty -->
  <div v-if="!account" class="wrapper">No account found.</div>

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
        <IconUpload
          v-model="form.icon"
          :fallback-text="displayInitial"
          aria-label="Click to upload account icon"
        />

        <ItemField
          class="display-name"
          label="display name"
          type="text"
          input
          v-model="form.display_name"
          placeholder="No Value"
        />
      </section>

      <section class="item-fields-section">
        <!-- Username -->
        <ItemField
          label="username"
          type="text"
          input
          v-model="form.username"
          placeholder="No Value"
        />

        <!-- Email -->
        <ItemField
          label="email"
          type="email"
          input
          v-model="form.email"
          placeholder="No Value"
        />

        <!-- Password (part of the shared section; emptying it removes the value) -->
        <ItemField
          label="password"
          :type="passwordFocused ? 'text' : 'password'"
          input
          v-model="passwordInput"
          :placeholder="passwordPlaceholder"
          @focus="passwordFocused = true"
          @blur="passwordFocused = false"
        >
          <template #actions>
            <span v-if="passwordEntropy" class="password-strength">
              {{ PASSWORDSTRENGTHS[passwordEntropy.score] }}
            </span>
          </template>
        </ItemField>
      </section>

      <!-- Failed password prefetch: the stored state is unknown, so save
           pushes metadata only and never the password -->
      <p v-if="passwordFetchFailed" class="password-fetch-error" role="alert">
        Couldn't load the stored password — password changes were not saved.
      </p>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList v-model="form.tags" editable />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(account.updated_at)} ` }}</span>
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
  flex: 1;
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

.display-name {
  border-radius: 0.75rem;
  flex: 1;
}

.item-fields-section {
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

.password-strength {
  margin-right: 0.5rem;
  font-size: 0.875rem;
  font-weight: 500;
  color: var(--color-green);
  white-space: nowrap;
}

.password-fetch-error {
  font-size: 0.875rem;
  color: var(--color-red);
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
