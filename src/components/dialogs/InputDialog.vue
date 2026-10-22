<script setup lang="ts">
import { ref, onMounted, nextTick } from 'vue';
import BaseDialog from './BaseDialog.vue';

const props = defineProps<{
  title: string;
  label: string;
  initialValue?: string;
  confirmLabel: string;
}>();

const emit = defineEmits<{ submit: [value: string]; cancel: [] }>();

const value = ref(props.initialValue ?? '');
const inputRef = ref<HTMLInputElement | null>(null);

onMounted(async () => {
  await nextTick();
  const input = inputRef.value;
  if (!input) return;
  input.focus();
  // Select the name without its extension, as file managers do.
  const dot = value.value.lastIndexOf('.');
  input.setSelectionRange(0, dot > 0 ? dot : value.value.length);
});

function onSubmit() {
  const trimmed = value.value.trim();
  if (trimmed) emit('submit', trimmed);
}
</script>

<template>
  <BaseDialog :title="title" @cancel="emit('cancel')">
    <form class="form-group" @submit.prevent="onSubmit">
      <label class="form-label">{{ label }}</label>
      <input ref="inputRef" v-model="value" type="text" class="form-input" autocomplete="off" spellcheck="false" />
    </form>
    <template #footer>
      <button class="btn" @click="emit('cancel')">Annuler</button>
      <button class="btn btn-primary" :disabled="!value.trim()" @click="onSubmit">{{ confirmLabel }}</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.form-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
</style>
