<script setup lang="ts">
import { ref, onMounted } from 'vue';
import BaseDialog from './BaseDialog.vue';
import type { FileEntry, PaneSide } from '../../types';

defineProps<{ entries: FileEntry[]; side: PaneSide; destDir: string }>();
const emit = defineEmits<{ confirm: []; cancel: [] }>();

const MAX_LISTED = 8;
const confirmRef = ref<HTMLButtonElement | null>(null);

onMounted(() => confirmRef.value?.focus());
</script>

<template>
  <BaseDialog :title="side === 'local' ? 'Déplacer vers le serveur' : 'Déplacer vers l’ordinateur'" :width="440" @cancel="emit('cancel')">
    <p>
      Déplacer {{ entries.length === 1 ? 'cet élément' : `ces ${entries.length} éléments` }} vers
      <span class="mono">{{ destDir }}</span> ?
    </p>
    <ul class="entry-list">
      <li v-for="entry in entries.slice(0, MAX_LISTED)" :key="entry.path">
        {{ entry.is_symlink ? '🔗' : entry.is_dir ? '📁' : '📄' }} {{ entry.name }}
      </li>
      <li v-if="entries.length > MAX_LISTED" class="muted">… et {{ entries.length - MAX_LISTED }} autre(s)</li>
    </ul>
    <p class="muted">
      Les originaux sont supprimés {{ side === 'local' ? 'de cet ordinateur' : 'du serveur' }} une fois copiés.
      Si un seul fichier d’un dossier n’a pas pu être copié, rien n’est supprimé.
    </p>
    <template #footer>
      <button class="btn" @click="emit('cancel')">Annuler</button>
      <button ref="confirmRef" class="btn btn-primary" @click="emit('confirm')">Déplacer</button>
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
