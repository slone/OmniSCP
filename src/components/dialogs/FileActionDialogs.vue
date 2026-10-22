<script setup lang="ts">
import { useFileActionsStore } from '../../stores/fileActions';
import InputDialog from './InputDialog.vue';
import ConfirmDeleteDialog from './ConfirmDeleteDialog.vue';
import ConflictDialog from './ConflictDialog.vue';

const actions = useFileActionsStore();
</script>

<template>
  <template v-if="actions.dialog">
    <InputDialog
      v-if="actions.dialog.type === 'mkdir'"
      :title="actions.dialog.side === 'local' ? 'Nouveau dossier local' : 'Nouveau dossier distant'"
      label="Nom du dossier"
      confirm-label="Créer"
      @submit="(name) => actions.dialog?.type === 'mkdir' && actions.mkdir(actions.dialog.side, name)"
      @cancel="actions.closeDialog"
    />
    <InputDialog
      v-else-if="actions.dialog.type === 'rename'"
      title="Renommer"
      label="Nouveau nom"
      :initial-value="actions.dialog.entry.name"
      confirm-label="Renommer"
      @submit="(name) => actions.dialog?.type === 'rename' && actions.rename(actions.dialog.side, actions.dialog.entry, name)"
      @cancel="actions.closeDialog"
    />
    <ConfirmDeleteDialog
      v-else-if="actions.dialog.type === 'delete'"
      :entries="actions.dialog.entries"
      :remote="actions.dialog.side === 'remote'"
      @confirm="actions.dialog?.type === 'delete' && actions.remove(actions.dialog.side, actions.dialog.entries)"
      @cancel="actions.closeDialog"
    />
    <ConflictDialog
      v-else-if="actions.dialog.type === 'conflict'"
      :name="actions.dialog.name"
      :remaining="actions.dialog.remaining"
      @answer="actions.answerConflict"
    />
  </template>
</template>
