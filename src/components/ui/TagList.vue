<script setup lang="ts">
import { Tag } from '@lucide/vue';

interface Props {
  tags: string[];
}

const props = defineProps<Props>();
</script>

<template>
  <span v-if="tags.length < 1" class="empty"> No tags. </span>

  <ul v-else class="tag-list">
    <li v-for="tag in props.tags">
      <Tag :size="16" />
      <span>{{ tag }}</span>
    </li>
  </ul>
</template>

<style scoped>
.empty {
  line-height: 2rem;
  color: var(--color-text-muted);
  font-size: 0.875rem;
}

.tag-list {
  display: flex;
  flex-wrap: wrap;
  gap: 0.5rem;
  list-style: none;

  li {
    display: flex;
    align-items: center;
    box-sizing: border-box;
    gap: 0.375rem;

    max-height: 2rem;

    padding: 0.75rem;

    background-color: var(--color-bg);
    border: 1px solid var(--color-border);
    border-radius: 1.5rem;

    font-size: 0.875rem;

    text-box-trim: trim-both;
    text-box-edge: cap alphabetic;

    > svg {
      color: var(--color-text-muted);
    }
  }
}
</style>
