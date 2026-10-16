<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { useRoute, useRouter } from 'vue-router';
import { ChevronRight } from '@lucide/vue';
import Button from '../ui/Button.vue';
import TagList from '../ui/TagList.vue';
import { formatTimestamp } from '../../util/timestamp.ts';
import AccountField from '../ui/AccountField.vue';
import IconUpload from '../ui/IconUpload.vue';
import useAppStore from '../../stores/appStore.ts';

const route = useRoute();
const router = useRouter();
const { state, updateActiveItem } = useAppStore();

const identity = computed(() =>
  state.activeItem?.kind === 'identity' ? state.activeItem : null
);

const displayLabel = computed(
  () => identity.value?.display_name || identity.value?.full_name || 'Untitled'
);

const displayInitial = computed(() => displayLabel.value[0].toUpperCase());

interface IdentityForm {
  icon: string | null;
  full_name: string;
  email: string | null;
  phone: string | null;
  address_line1: string | null;
  address_line2: string | null;
  city: string | null;
  postal_code: string | null;
  country: string | null;
  tags: string[];
}

const form = ref<IdentityForm>({
  icon: null,
  full_name: '',
  email: null,
  phone: null,
  address_line1: null,
  address_line2: null,
  city: null,
  postal_code: null,
  country: null,
  tags: []
});

// Sync local form state whenever the globally active identity changes
watch(
  () => identity.value,
  (nextIdentity) => {
    if (nextIdentity) {
      form.value = {
        icon: nextIdentity.icon,
        full_name: nextIdentity.full_name,
        email: nextIdentity.email,
        phone: nextIdentity.phone,
        address_line1: nextIdentity.address_line1,
        address_line2: nextIdentity.address_line2,
        city: nextIdentity.city,
        postal_code: nextIdentity.postal_code,
        country: nextIdentity.country,
        tags: [...nextIdentity.tags]
      };
    }
  },
  { immediate: true }
);

function navigateToDetails() {
  router.push({
    name: route.name as string,
    params: { ...route.params, mode: undefined },
    query: route.query
  });
}

async function handleSave() {
  if (!identity.value) return;

  const success = await updateActiveItem({
    icon: form.value.icon,
    tags: form.value.tags,
    data: {
      kind: 'identity',
      full_name: form.value.full_name,
      email: form.value.email,
      phone: form.value.phone,
      address_line1: form.value.address_line1,
      address_line2: form.value.address_line2,
      city: form.value.city,
      postal_code: form.value.postal_code,
      country: form.value.country
    }
  });

  if (success) navigateToDetails();
}
</script>

<template>
  <!-- Empty -->
  <div v-if="!identity" class="wrapper">No identity found.</div>

  <!-- Edit Mode -->
  <div v-else class="wrapper">
    <header>
      <span class="editmode-label">Editing</span>

      <nav class="header-toolbar">
        <Button variant="solid" size="small" @click="navigateToDetails">Cancel</Button>
        <Button size="small" @click="handleSave">Save</Button>
      </nav>
    </header>

    <main class="thin-scrollbar">
      <section class="descriptor-section">
        <IconUpload
          v-model="form.icon"
          :fallback-text="displayInitial"
          aria-label="Click to upload identity icon"
        />
      </section>

      <section class="identity-fields-section">
        <!-- Full Name -->
        <AccountField
          label="full name"
          type="text"
          required
          input
          v-model="form.full_name"
          placeholder="Full Name"
        />

        <!-- Email -->
        <AccountField
          label="email"
          type="email"
          input
          v-model="form.email"
          placeholder="email@example.com"
        />

        <!-- Phone -->
        <AccountField
          label="phone"
          type="tel"
          input
          v-model="form.phone"
          placeholder="+1 555 0000"
        />

        <!-- Address -->
        <AccountField
          label="address line 1"
          type="text"
          input
          v-model="form.address_line1"
          placeholder="Street Address"
        />

        <AccountField
          label="address line 2"
          type="text"
          input
          v-model="form.address_line2"
          placeholder="Apartment, suite, unit…"
        />

        <AccountField label="city" type="text" input v-model="form.city" placeholder="City" />

        <AccountField
          label="postal code"
          type="text"
          input
          v-model="form.postal_code"
          placeholder="Postal Code"
        />

        <AccountField
          label="country"
          type="text"
          input
          v-model="form.country"
          placeholder="Country"
        />
      </section>

      <section class="tags-section">
        <h2>tags</h2>

        <TagList v-model="form.tags" editable />
      </section>

      <section class="timestamp-section">
        <ChevronRight :size="20" />
        <span>{{ `Last edited ${formatTimestamp(identity.updated_at)} ` }}</span>
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

.identity-fields-section {
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
