<script setup lang="ts">
import { useFileActionsStore } from '../../stores/fileActions';
import InputDialog from './InputDialog.vue';
import ConfirmDeleteDialog from './ConfirmDeleteDialog.vue';
import ConflictDialog from './ConflictDialog.vue';
import PropertiesDialog from './PropertiesDialog.vue';
import ConfirmMoveDialog from './ConfirmMoveDialog.vue';
import { t } from '../../i18n';

const actions = useFileActionsStore();

function onMove() {
  const dialog = actions.dialog;
  if (dialog?.type !== 'move') return;
  actions.closeDialog();
  actions.transfer(dialog.side, dialog.entries, true);
}
</script>

<template>
  <template v-if="actions.dialog">
    <InputDialog
      v-if="actions.dialog.type === 'mkdir'"
      :title="actions.dialog.side === 'local' ? t.dialogs.mkdirLocal : t.dialogs.mkdirRemote"
      :label="t.dialogs.folderName"
      :confirm-label="t.dialogs.create"
      @submit="(name) => actions.dialog?.type === 'mkdir' && actions.mkdir(actions.dialog.side, name)"
      @cancel="actions.closeDialog"
    />
    <InputDialog
      v-else-if="actions.dialog.type === 'rename'"
      :title="t.dialogs.rename"
      :label="t.dialogs.newName"
      :initial-value="actions.dialog.entry.name"
      :confirm-label="t.dialogs.rename"
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
    <PropertiesDialog
      v-else-if="actions.dialog.type === 'properties'"
      :side="actions.dialog.side"
      :entries="actions.dialog.entries"
      @close="actions.closeDialog"
    />
    <ConfirmMoveDialog
      v-else-if="actions.dialog.type === 'move'"
      :entries="actions.dialog.entries"
      :side="actions.dialog.side"
      :dest-dir="actions.dialog.destDir"
      @confirm="onMove"
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
