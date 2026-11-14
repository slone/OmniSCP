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
import type { EntryMeta, SyncAction, SyncActionKind, SyncMode, SyncPlan } from '../../types';
import type { BackendError } from '../../utils/errors';
import { t } from '../../i18n';

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
const problems = ref<BackendError[]>([]);
const skippedLinks = ref<string[]>([]);
const error = ref('');

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
  return meta.is_dir ? t.sync.folder : `${formatSize(meta.size)} · ${formatDate(meta.modified)}`;
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
    logStore.addLog('Info', t.log.sync, t.sync.compared, `${localRoot} ⇄ ${remoteRoot} — ${t.sync.actions(plan.actions.length)}`);
  } catch (err) {
    phase.value = 'options';
    if (isBackendError(err) && err.kind === 'Cancelled') return;
    error.value = errorMessage(err);
    logStore.addLog('Error', t.log.sync, t.log.failed, `${localRoot} ⇄ ${remoteRoot} — ${error.value}`);
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
  <BaseDialog :title="t.sync.title" :width="780" @cancel="close">
    <div class="roots">
      <div><span class="form-label">{{ t.sync.local }}</span> {{ localRoot }}</div>
      <div><span class="form-label">{{ t.sync.server }}</span> {{ remoteRoot }}</div>
    </div>

    <template v-if="phase === 'options' || phase === 'comparing'">
      <fieldset class="modes" :disabled="phase === 'comparing'">
        <legend class="visually-hidden">{{ t.sync.direction }}</legend>
        <label><input v-model="mode" type="radio" value="Upload" /> {{ t.sync.upload }}</label>
        <label><input v-model="mode" type="radio" value="Download" /> {{ t.sync.download }}</label>
        <label><input v-model="mode" type="radio" value="Both" /> {{ t.sync.both }}</label>
      </fieldset>
      <p class="hint">
        {{ mode === 'Both' ? t.sync.bothHint : t.sync.oneWayHint }}
      </p>

      <label class="form-label" for="sync-excludes">{{ t.sync.excludes }}</label>
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
          {{ t.sync.deleteOrphans }}
        </label>
        <p v-if="deleteOrphans" class="warning">
          {{ t.sync.deleteOrphansHint }}
        </p>
      </template>

      <p v-if="error" class="error" role="alert">{{ error }}</p>
      <p v-if="phase === 'comparing'" class="hint" role="status">{{ t.sync.comparing }}</p>
    </template>

    <template v-else-if="phase === 'preview'">
      <p v-if="rows.length === 0" class="hint" role="status">{{ t.sync.nothing }}</p>
      <div v-else class="plan">
        <table>
          <thead>
            <tr>
              <th><span class="visually-hidden">{{ t.sync.include }}</span></th>
              <th>{{ t.sync.action }}</th>
              <th>{{ t.sync.path }}</th>
              <th>{{ t.sync.local }}</th>
              <th>{{ t.sync.server }}</th>
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
                  :aria-label="t.sync.resolve(row.path)"
                >
                  <option value="Skip">{{ t.sync.skip }}</option>
                  <option value="Upload">{{ t.sync.send }}</option>
                  <option value="Download">{{ t.sync.fetch }}</option>
                </select>
                <input v-else v-model="row.selected" type="checkbox" :aria-label="row.path" />
              </td>
              <td class="action">
                {{ t.sync.kinds[row.kind] }}
                <div class="reason">{{ t.sync.reasons[row.reason] }}</div>
              </td>
              <td class="path" :title="row.path">{{ row.path }}</td>
              <td class="meta">{{ describe(row.local) }}</td>
              <td class="meta">{{ describe(row.remote) }}</td>
            </tr>
          </tbody>
        </table>
      </div>
      <div v-if="problems.length > 0" class="warning" role="alert">
        {{ t.sync.unreadable }}
        <ul>
          <li v-for="(p, i) in problems" :key="i">{{ errorMessage(p) }}</li>
        </ul>
      </div>
      <p v-if="skippedLinks.length > 0" class="hint">
        {{ t.sync.skippedLinks(skippedLinks.length) }}
        {{ skippedLinks.slice(0, 5).join(', ') }}{{ skippedLinks.length > 5 ? '…' : '' }}
      </p>
    </template>

    <template v-else>
      <p>
        <strong>{{ t.sync.deletions(deletions.length) }}</strong>
        {{ t.sync.irreversible }}
      </p>
      <ul class="deletions">
        <li v-for="a in deletions.slice(0, 20)" :key="a.path">{{ t.sync.kinds[a.kind] }} — {{ a.path }}</li>
        <li v-if="deletions.length > 20">{{ t.common.more(deletions.length - 20) }}</li>
      </ul>
    </template>

    <template #footer>
      <button v-if="phase === 'comparing'" class="btn" @click="stopComparing">{{ t.sync.stopComparing }}</button>
      <button v-else class="btn" @click="close">{{ t.common.close }}</button>

      <button v-if="phase === 'options'" class="btn btn-primary" @click="compare">{{ t.sync.compare }}</button>

      <template v-if="phase === 'preview'">
        <button class="btn" @click="phase = 'options'">{{ t.sync.changeOptions }}</button>
        <button class="btn btn-primary" :disabled="toRun.length === 0" @click="run">
          {{ t.sync.run(toRun.length) }}
        </button>
      </template>

      <template v-if="phase === 'confirm'">
        <button class="btn" @click="phase = 'preview'">{{ t.sync.backToPreview }}</button>
        <button class="btn btn-danger" @click="run">{{ t.sync.deleteAndSync }}</button>
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
  color: var(--color-warning);
}

.warning ul {
  margin-top: 4px;
  padding-left: 16px;
  max-height: 80px;
  overflow-y: auto;
}

.error {
  color: var(--color-danger);
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
  color: var(--color-danger);
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
