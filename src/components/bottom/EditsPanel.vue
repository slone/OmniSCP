<script setup lang="ts">
import { useEditsStore, type EditedFile } from '../../stores/edits';
import { formatTime } from '../../utils/format';
import { t } from '../../i18n';

const edits = useEditsStore();

function stateLabel(file: EditedFile) {
  switch (file.state) {
    case 'sending':
      return t.edits.sending;
    case 'conflict':
      return t.edits.conflict;
    case 'error':
      return t.edits.error;
    default:
      return file.sentAt ? t.edits.sentAt(formatTime(file.sentAt)) : t.edits.watching;
  }
}
</script>

<template>
  <div class="edits">
    <p class="hint">
      {{ t.edits.hint }}
    </p>
    <table class="edits-table">
      <tbody>
        <tr v-for="file in edits.files" :key="file.id" :class="'state-' + file.state">
          <td class="path" :title="file.local_path">{{ file.remote_path }}</td>
          <td class="state" role="status">
            {{ stateLabel(file) }}
            <div v-if="file.error" class="state-error">{{ file.error }}</div>
          </td>
          <td class="actions">
            <template v-if="file.state === 'conflict'">
              <button class="edit-btn danger" :title="t.edits.overwriteHint" @click="edits.send(file.id, true)">
                {{ t.edits.overwrite }}
              </button>
              <button class="edit-btn" :title="t.edits.dismissHint" @click="edits.dismiss(file.id)">{{ t.edits.dismiss }}</button>
            </template>
            <button
              v-else
              class="edit-btn"
:title="t.edits.sendHint"
              :disabled="file.state === 'sending'"
              @click="edits.send(file.id, false)"
            >
              {{ t.edits.send }}
            </button>
            <button class="edit-btn" :title="t.edits.stopHint" @click="edits.stop(file.id)">
              {{ t.edits.stop }}
            </button>
          </td>
        </tr>
      </tbody>
    </table>
  </div>
</template>

<style scoped>
.edits {
  height: 100%;
  overflow-y: auto;
  font-size: 12px;
}

.hint {
  margin: 0;
  padding: 4px 8px;
  border-bottom: 1px solid var(--color-border);
  color: var(--color-text-muted);
  font-size: 11px;
}

.edits-table {
  width: 100%;
  border-collapse: collapse;
}

.edits-table td {
  padding: 4px 8px;
  white-space: nowrap;
}

.path {
  max-width: 400px;
  overflow: hidden;
  text-overflow: ellipsis;
  font-family: var(--font-mono);
  font-size: 11px;
}

.state {
  color: var(--color-text-muted);
}

.state-error {
  white-space: normal;
  font-size: 11px;
}

.state-conflict .state,
.state-error .state {
  color: var(--color-danger);
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 4px;
}

.edit-btn {
  padding: 1px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: none;
  color: var(--color-text);
  font-size: 11px;
  cursor: pointer;
}

.edit-btn:hover:not(:disabled) {
  background: var(--color-hover);
}

.edit-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.edit-btn.danger {
  border-color: var(--color-danger-border);
  color: var(--color-danger-text);
}
</style>
