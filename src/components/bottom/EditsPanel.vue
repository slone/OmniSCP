<script setup lang="ts">
import { useEditsStore, type EditedFile } from '../../stores/edits';

const edits = useEditsStore();

function stateLabel(file: EditedFile) {
  switch (file.state) {
    case 'sending':
      return 'Envoi…';
    case 'conflict':
      return 'Modifié sur le serveur';
    case 'error':
      return 'Échec de l’envoi';
    default:
      return file.sentAt
        ? `Envoyé à ${new Date(file.sentAt).toLocaleTimeString('fr-FR')}`
        : 'Surveillé — enregistrez pour envoyer';
  }
}
</script>

<template>
  <div class="edits">
    <p class="hint">
      Chaque enregistrement dans l’application est envoyé sur le serveur, sauf si le fichier y a changé entre-temps.
    </p>
    <table class="edits-table">
      <tbody>
        <tr v-for="file in edits.files" :key="file.id" :class="'state-' + file.state">
          <td class="path" :title="file.local_path">{{ file.remote_path }}</td>
          <td class="state" :title="file.error">{{ stateLabel(file) }}</td>
          <td class="actions">
            <template v-if="file.state === 'conflict'">
              <button class="edit-btn danger" title="Remplacer la version du serveur par la vôtre" @click="edits.send(file.id, true)">
                Écraser
              </button>
              <button class="edit-btn" title="Ne pas envoyer cette modification" @click="edits.dismiss(file.id)">Ignorer</button>
            </template>
            <button
              v-else
              class="edit-btn"
              title="Envoyer la copie locale maintenant"
              :disabled="file.state === 'sending'"
              @click="edits.send(file.id, false)"
            >
              Envoyer
            </button>
            <button class="edit-btn" title="Ne plus surveiller ce fichier ; la copie locale est conservée" @click="edits.stop(file.id)">
              Arrêter
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
