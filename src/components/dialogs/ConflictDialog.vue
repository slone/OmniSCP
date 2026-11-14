<script setup lang="ts">
import { ref } from 'vue';
import BaseDialog from './BaseDialog.vue';
import type { ConflictChoice } from '../../stores/fileActions';
import { t } from '../../i18n';

defineProps<{ name: string; remaining: number }>();
const emit = defineEmits<{ answer: [choice: ConflictChoice, applyToAll: boolean] }>();

const applyToAll = ref(false);
</script>

<template>
  <BaseDialog :title="t.dialogs.conflictTitle" :width="420" @cancel="emit('answer', 'Cancel', false)">
    <p>{{ t.dialogs.conflictText(name) }}</p>
    <label v-if="remaining > 0" class="apply-all">
      <input v-model="applyToAll" type="checkbox" />
      {{ t.dialogs.applyToAll(remaining) }}
    </label>
    <template #footer>
      <button class="btn" @click="emit('answer', 'Cancel', false)">{{ t.common.cancel }}</button>
      <button class="btn" @click="emit('answer', 'Skip', applyToAll)">{{ t.dialogs.skip }}</button>
      <button class="btn" @click="emit('answer', 'Rename', applyToAll)">{{ t.dialogs.rename }}</button>
      <button class="btn btn-primary" @click="emit('answer', 'Overwrite', applyToAll)">{{ t.dialogs.overwrite }}</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.apply-all {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}
</style>
