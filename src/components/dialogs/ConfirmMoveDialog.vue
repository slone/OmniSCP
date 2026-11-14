<script setup lang="ts">
import { ref, onMounted } from 'vue';
import BaseDialog from './BaseDialog.vue';
import type { FileEntry, PaneSide } from '../../types';
import { t } from '../../i18n';
import EntryIcon from './EntryIcon.vue';

defineProps<{ entries: FileEntry[]; side: PaneSide; destDir: string }>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();

const MAX_LISTED = 8;
const confirmRef = ref<HTMLButtonElement | null>(null);

onMounted(() => confirmRef.value?.focus());
</script>

<template>
  <BaseDialog :title="side === 'local' ? t.dialogs.moveToServer : t.dialogs.moveToComputer" :width="440" @cancel="emit('cancel')">
    <p>
      {{ t.dialogs.moveText(entries.length) }}
      <span class="mono">{{ destDir }}</span> ?
    </p>
    <ul class="entry-list">
      <li v-for="entry in entries.slice(0, MAX_LISTED)" :key="entry.path">
        <EntryIcon :entry="entry" /> {{ entry.name }}
      </li>
      <li v-if="entries.length > MAX_LISTED" class="muted">{{ t.common.more(entries.length - MAX_LISTED) }}</li>
    </ul>
    <p class="muted">{{ side === 'local' ? t.dialogs.moveNoteLocal : t.dialogs.moveNoteRemote }}</p>
    <template #footer>
      <button class="btn" @click="emit('cancel')">{{ t.common.cancel }}</button>
      <button ref="confirmRef" class="btn btn-primary" @click="emit('confirm')">{{ t.dialogs.move }}</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
p {
  margin: 0;
}

.mono,
.entry-list {
  font-family: var(--font-mono);
}

.entry-list {
  list-style: none;
  margin: 0;
  max-height: 160px;
  overflow-y: auto;
  padding: 6px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
}

.muted {
  color: var(--color-text-muted);
}
</style>
