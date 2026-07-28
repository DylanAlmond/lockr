<script setup lang="ts">
import { ref, nextTick } from 'vue';
import { Tag, Plus, X } from '@lucide/vue';

interface Props {
  modelValue?: string[];
  editable?: boolean;
}

const props = withDefaults(defineProps<Props>(), {
  modelValue: () => [],
  editable: false
});

const emit = defineEmits<{
  (e: 'update:modelValue', tags: string[]): void;
}>();

const isAdding = ref(false);
const newTag = ref('');
const inputRef = ref<HTMLInputElement | null>(null);

function startAdding() {
  isAdding.value = true;
  nextTick(() => {
    inputRef.value?.focus();
  });
}

function addTag() {
  const trimmed = newTag.value.trim();
  if (trimmed && !props.modelValue.includes(trimmed)) {
    emit('update:modelValue', [...props.modelValue, trimmed]);
  }
  newTag.value = '';
  isAdding.value = false;
}

function cancelAdd() {
  newTag.value = '';
  isAdding.value = false;
}

function removeTag(index: number) {
  const newTags = [...props.modelValue];
  newTags.splice(index, 1);
  emit('update:modelValue', newTags);
}
</script>

<template>
  <span v-if="!editable && modelValue.length < 1" class="empty"> No tags. </span>

  <ul v-else class="tag-list">
    <li v-for="(tag, index) in modelValue" :key="tag" class="tag-item">
      <button v-if="editable" class="remove-btn" @click="removeTag(index)" aria-label="Remove tag">
        <X :size="14" />
      </button>

      <Tag v-else :size="16" />

      <span>{{ tag }}</span>
    </li>

    <li v-if="editable" class="add-tag-container">
      <input
        v-if="isAdding"
        ref="inputRef"
        v-model="newTag"
        type="text"
        class="tag-input"
        placeholder="Tag name..."
        @keyup.enter="addTag"
        @keyup.esc="cancelAdd"
        @blur="cancelAdd"
      />
      <button v-else class="add-btn" @click="startAdding" aria-label="Add tag">
        <Plus :size="16" />
      </button>
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
  padding: 0;
  margin: 0;
}

.tag-item {
  display: flex;
  align-items: center;
  box-sizing: border-box;
  gap: 0.375rem;

  max-height: 2rem;

  padding: 0.5rem 0.75rem;

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

.remove-btn {
  appearance: none;
  border: none;
  outline: none;
  background: transparent;
  padding: 0;
  margin-left: 0.125rem;
  display: flex;
  align-items: center;
  color: var(--color-text-muted);
  cursor: pointer;
  transition: color 0.2s ease;

  &:hover {
    color: var(--color-danger, #ef4444);
  }
}

.add-tag-container {
  display: flex;
  align-items: center;
  max-height: 2rem;
}

.add-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  box-sizing: border-box;

  width: 2rem;
  height: 2rem;

  background-color: transparent;
  border: 1px dashed var(--color-border);
  border-radius: 1.5rem;

  color: var(--color-text-muted);
  cursor: pointer;

  transition: all 0.2s ease;

  &:hover {
    border-color: var(--color-accent);
    color: var(--color-accent);
    border-style: solid;
  }
}

.tag-input {
  box-sizing: border-box;
  max-height: 2rem;

  padding: 0.5rem 0.75rem;

  background-color: var(--color-bg);
  border: 1px solid var(--color-accent);
  border-radius: 1.5rem;

  font-size: 0.875rem;
  font-family: inherit;
  color: var(--color-text-tertiary);

  outline: none;
  width: 120px;

  &::placeholder {
    color: var(--color-text-muted);
  }
}
</style>
