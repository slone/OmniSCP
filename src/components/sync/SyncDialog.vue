<script setup lang="ts">
import { computed, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BaseDialog from '../dialogs/BaseDialog.vue';
import { useConnectionStore } from '../../stores/connection';
import { useLocalFilesStore } from '../../stores/localFiles';
import { useLogStore } from '../../stores/logs';
import { useTransfersStore } from '../../stores/transfers';
import { errorMessage, isBackendError } from '../../utils/errors';
import { formatDate, formatSize } from '../../utils/format';
import type { EntryMeta, SyncAction, SyncActionKind, SyncMode, SyncPlan, SyncReason } from '../../types';

const emit = defineEmits<{ close: [] }>();

const connection = useConnectionStore();
const logStore = useLogStore();
const transfers = useTransfersStore();

// The panes can keep browsing; the sync stays on the folders it was opened on.
const localRoot = useLocalFilesStore().currentPath;
const remoteRoot = connection.currentPath;

type ConflictChoice = 'Skip' | 'Upload' | 'Download';
interface PlanRow extends SyncAction {
  choice: ConflictChoice;
}

const phase = ref<'options' | 'comparing' | 'preview' | 'confirm'>('options');
const mode = ref<SyncMode>('Upload');
const excludesText = ref('');
const deleteOrphans = ref(false);
const rows = ref<PlanRow[]>([]);
const problems = ref<string[]>([]);
const skippedLinks = ref<string[]>([]);
const error = ref('');

const kindLabels: Record<SyncActionKind, string> = {
  Upload: '↑ Envoyer',
  Download: '↓ Récupérer',
  CreateRemoteDir: '📁 Créer sur le serveur',
  CreateLocalDir: '📁 Créer en local',
  DeleteRemote: '🗑 Supprimer sur le serveur',
  DeleteLocal: '🗑 Supprimer en local',
  Conflict: '⚠ Conflit',
};

const reasonLabels: Record<SyncReason, string> = {
  New: 'nouveau',
  SourceNewer: 'plus récent',
  TargetNewer: 'la destination est plus récente',
  SameDateDifferentSize: 'même date, taille différente',
  TypeMismatch: "fichier d'un côté, dossier de l'autre",
  Orphan: 'absent de la source',
};

// The same box would delete on the other side after a change of direction.
watch(mode, () => {
  deleteOrphans.value = false;
});

watch(
  () => connection.connected,
  (connected) => {
    if (!connected) emit('close');
  },
);

const isDelete = (kind: SyncActionKind) => kind === 'DeleteLocal' || kind === 'DeleteRemote';

/// What will really run: ticked rows, and conflicts the user settled.
const toRun = computed<SyncAction[]>(() =>
  rows.value.flatMap(({ choice, ...action }) => {
    if (action.kind === 'Conflict') {
      return choice === 'Skip' ? [] : [{ ...action, kind: choice, selected: true }];
    }
    return action.selected ? [action] : [];
  }),
);
const deletions = computed(() => toRun.value.filter((a) => isDelete(a.kind)));

function describe(meta: EntryMeta | null): string {
  if (!meta) return '—';
  return meta.is_dir ? 'dossier' : `${formatSize(meta.size)} · ${formatDate(meta.modified)}`;
}

async function compare() {
  error.value = '';
  phase.value = 'comparing';
  const excludes = excludesText.value
    .split(/[,\n]/)
    .map((p) => p.trim())
    .filter((p) => p.length > 0);
  try {
    const plan = await invoke<SyncPlan>('compare_sync', {
      localRoot,
      remoteRoot,
      options: { mode: mode.value, excludes, delete_orphans: deleteOrphans.value },
    });
    rows.value = plan.actions.map((a) => ({ ...a, choice: 'Skip' }));
    problems.value = plan.problems;
    skippedLinks.value = plan.skipped_links;
    phase.value = 'preview';
    logStore.addLog('Info', 'Synchronisation', 'Comparé', `${localRoot} ⇄ ${remoteRoot} — ${plan.actions.length} action(s)`);
  } catch (err) {
    phase.value = 'options';
    if (isBackendError(err) && err.kind === 'Cancelled') return;
    error.value = errorMessage(err);
    logStore.addLog('Error', 'Synchronisation', 'Échec', `${localRoot} ⇄ ${remoteRoot} — ${error.value}`);
  }
}

function close() {
  if (phase.value === 'comparing') {
    invoke('cancel_sync_compare').catch(() => {});
  }
  emit('close');
}

function stopComparing() {
  invoke('cancel_sync_compare').catch(() => {});
}

async function run() {
  if (deletions.value.length > 0 && phase.value !== 'confirm') {
    phase.value = 'confirm';
    return;
  }
  await transfers.enqueueSync(localRoot, remoteRoot, toRun.value);
  emit('close');
}
</script>

<template>
  <BaseDialog title="Synchroniser les dossiers" :width="780" @cancel="close">
    <div class="roots">
      <div><span class="form-label">Local</span> {{ localRoot }}</div>
      <div><span class="form-label">Serveur</span> {{ remoteRoot }}</div>
    </div>

    <template v-if="phase === 'options' || phase === 'comparing'">
      <fieldset class="modes" :disabled="phase === 'comparing'">
        <label><input v-model="mode" type="radio" value="Upload" /> Envoyer vers le serveur</label>
        <label><input v-model="mode" type="radio" value="Download" /> Récupérer depuis le serveur</label>
        <label><input v-model="mode" type="radio" value="Both" /> Dans les deux sens</label>
      </fieldset>
      <p class="hint">
        <template v-if="mode === 'Both'">
          Le fichier le plus récent remplace l'autre. Rien n'est supprimé : un fichier absent d'un côté est copié.
        </template>
        <template v-else>
          Seule la destination est modifiée. Un fichier plus récent à la destination n'est remplacé que si vous le cochez.
        </template>
      </p>

      <label class="form-label" for="sync-excludes">Exclusions</label>
      <input
        id="sync-excludes"
        v-model="excludesText"
        class="form-input"
        type="text"
        placeholder="*.tmp, node_modules, build/*.o"
        :disabled="phase === 'comparing'"
      />

      <template v-if="mode !== 'Both'">
        <label class="checkbox">
          <input v-model="deleteOrphans" type="checkbox" :disabled="phase === 'comparing'" />
          Supprimer à la destination ce qui n'existe plus à la source
        </label>
        <p v-if="deleteOrphans" class="warning">
          Les suppressions seront listées dans l'aperçu et demanderont une confirmation.
        </p>
      </template>

      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <p v-if="phase === 'comparing'" class="hint">Analyse des deux dossiers en cours…</p>
    </template>

    <template v-else-if="phase === 'preview'">
      <p v-if="rows.length === 0" class="hint">Rien à faire : les dossiers sont déjà synchronisés.</p>
      <div v-else class="plan">
        <table>
          <thead>
            <tr>
              <th></th>
              <th>Action</th>
              <th>Chemin</th>
              <th>Local</th>
              <th>Serveur</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="row in rows" :key="row.path" :class="{ dangerous: isDelete(row.kind) }">
              <td>
                <select
                  v-if="row.kind === 'Conflict'"
                  v-model="row.choice"
                  class="choice"
                  :disabled="row.reason === 'TypeMismatch'"
                  :aria-label="`Résoudre le conflit sur ${row.path}`"
                >
                  <option value="Skip">Ignorer</option>
                  <option value="Upload">Envoyer</option>
                  <option value="Download">Récupérer</option>
                </select>
                <input v-else v-model="row.selected" type="checkbox" :aria-label="row.path" />
              </td>
              <td class="action">
                {{ kindLabels[row.kind] }}
                <div class="reason">{{ reasonLabels[row.reason] }}</div>
              </td>
              <td class="path" :title="row.path">{{ row.path }}</td>
              <td class="meta">{{ describe(row.local) }}</td>
              <td class="meta">{{ describe(row.remote) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
      <div v-if="problems.length > 0" class="warning" role="alert">
        Dossiers illisibles, ignorés (aucune action en dessous) :
        <ul>
          <li v-for="p in problems" :key="p">{{ p }}</li>
        </ul>
      </div>
      <p v-if="skippedLinks.length > 0" class="hint">
        {{ skippedLinks.length }} lien(s) vers des dossiers ignoré(s) :
        {{ skippedLinks.slice(0, 5).join(', ') }}{{ skippedLinks.length > 5 ? '…' : '' }}
      </p>
    </template>

    <template v-else>
      <p>
        <strong>{{ deletions.length }} élément(s) seront supprimés définitivement.</strong>
        Cette action est irréversible.
      </p>
      <ul class="deletions">
        <li v-for="a in deletions.slice(0, 20)" :key="a.path">{{ kindLabels[a.kind] }} — {{ a.path }}</li>
        <li v-if="deletions.length > 20">… et {{ deletions.length - 20 }} autre(s)</li>
      </ul>
    </template>

    <template #footer>
      <button v-if="phase === 'comparing'" class="btn" @click="stopComparing">Arrêter l'analyse</button>
      <button v-else class="btn" @click="close">Fermer</button>

      <button v-if="phase === 'options'" class="btn btn-primary" @click="compare">Comparer</button>

      <template v-if="phase === 'preview'">
        <button class="btn" @click="phase = 'options'">Modifier les options</button>
        <button class="btn btn-primary" :disabled="toRun.length === 0" @click="run">
          Exécuter ({{ toRun.length }})
        </button>
      </template>

      <template v-if="phase === 'confirm'">
        <button class="btn" @click="phase = 'preview'">Revenir à l'aperçu</button>
        <button class="btn btn-danger" @click="run">Supprimer et synchroniser</button>
      </template>
    </template>
  </BaseDialog>
</template>

<style scoped>
.roots {
  font-family: var(--font-mono);
  font-size: 11px;
  word-break: break-all;
}

.modes {
  display: flex;
  flex-wrap: wrap;
  gap: 16px;
  border: none;
}

.modes label,
.checkbox {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}

.hint {
  color: var(--color-text-muted);
}

.warning {
  color: #fbbf24;
}

.warning ul {
  margin-top: 4px;
  padding-left: 16px;
  max-height: 80px;
  overflow-y: auto;
}

.error {
  color: #f87171;
}

.plan {
  max-height: 44vh;
  overflow: auto;
  border: 1px solid var(--color-border);
}

.plan table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}

.plan thead {
  position: sticky;
  top: 0;
  background: var(--color-surface);
}

.plan th {
  padding: 4px 6px;
  text-align: left;
  font-weight: 500;
  color: var(--color-text-muted);
}

.plan td {
  padding: 3px 6px;
  border-top: 1px solid var(--color-border);
  vertical-align: top;
}

.action {
  white-space: nowrap;
}

.reason {
  color: var(--color-text-muted);
}

.path {
  font-family: var(--font-mono);
  word-break: break-all;
}

.meta {
  white-space: nowrap;
  color: var(--color-text-muted);
}

.dangerous .action {
  color: #f87171;
}

.choice {
  background: var(--color-bg);
  color: var(--color-text);
  border: 1px solid var(--color-border);
  font-size: 11px;
}

.deletions {
  list-style: none;
  max-height: 200px;
  overflow-y: auto;
  padding: 6px 8px;
  font-family: var(--font-mono);
  font-size: 11px;
  background: var(--color-bg);
  border: 1px solid var(--color-border);
}
</style>
