<script setup lang="ts">
import { ref, onMounted, computed } from 'vue';
import BaseDialog from './BaseDialog.vue';
import type { FileEntry } from '../../types';

const props = defineProps<{ entries: FileEntry[]; remote: boolean }>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();

const cancelRef = ref<HTMLButtonElement | null>(null);
const MAX_LISTED = 8;

const hasDirs = computed(() => props.entries.some((e) => e.is_dir && !e.is_symlink));
const listed = computed(() => props.entries.slice(0, MAX_LISTED));

onMounted(() => {
  // Focus the safe choice so Enter does not delete by accident.
  cancelRef.value?.focus();
});
</script>

<template>
  <BaseDialog :title="remote ? 'Supprimer sur le serveur' : 'Supprimer localement'" :width="420" @cancel="emit('cancel')">
    <p>
      Supprimer définitivement
      {{ entries.length === 1 ? 'cet élément' : `ces ${entries.length} éléments` }} ?
      Cette action est irréversible.
    </p>
    <ul class="entry-list">
      <li v-for="entry in listed" :key="entry.path">{{ entry.is_symlink ? '🔗' : entry.is_dir ? '📁' : '📄' }} {{ entry.name }}</li>
      <li v-if="entries.length > MAX_LISTED" class="more">… et {{ entries.length - MAX_LISTED }} autre(s)</li>
    </ul>
    <p v-if="hasDirs" class="warning">Les dossiers seront supprimés avec tout leur contenu.</p>
    <template #footer>
      <button ref="cancelRef" class="btn" @click="emit('cancel')">Annuler</button>
      <button class="btn btn-danger" @click="emit('confirm')">Supprimer</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.entry-list {
  list-style: none;
  max-height: 160px;
  overflow-y: auto;
  padding: 6px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  font-family: var(--font-mono);
}

.more {
  color: var(--color-text-muted);
}

.warning {
  color: var(--color-warning);
}
</style>
