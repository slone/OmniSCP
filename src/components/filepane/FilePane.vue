<script setup lang="ts">
import { ref, watch, computed } from 'vue';
import type { FileEntry, PaneSide } from '../../types';

const props = defineProps<{
  title: string;
  side: PaneSide;
  path: string;
  files: FileEntry[];
  selected: string[];
  active: boolean;
  loading?: boolean;
}>();

const emit = defineEmits<{
  navigate: [path: string];
  'navigate-up': [];
  'update:selected': [paths: string[]];
  activate: [];
  mkdir: [];
  rename: [];
  delete: [];
  transfer: [];
  /// Files dragged from the other pane were dropped here.
  drop: [from: PaneSide, paths: string[]];
}>();

const DRAG_TYPE = 'application/x-omniscp-files';

const editPath = ref(props.path);
const anchor = ref<string | null>(null);
const dropTarget = ref(false);

const selectedSet = computed(() => new Set(props.selected));

watch(() => props.path, (val) => {
  editPath.value = val;
  anchor.value = null;
});

function onPathKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    emit('navigate', editPath.value);
  }
}

function onRowClick(e: MouseEvent, file: FileEntry, index: number) {
  emit('activate');
  if (e.shiftKey && anchor.value) {
    const from = props.files.findIndex((f) => f.path === anchor.value);
    if (from >= 0) {
      const [lo, hi] = from < index ? [from, index] : [index, from];
      emit('update:selected', props.files.slice(lo, hi + 1).map((f) => f.path));
      return;
    }
  }
  if (e.metaKey || e.ctrlKey) {
    const next = selectedSet.value.has(file.path)
      ? props.selected.filter((p) => p !== file.path)
      : [...props.selected, file.path];
    emit('update:selected', next);
  } else {
    emit('update:selected', [file.path]);
  }
  anchor.value = file.path;
}

function onDblClick(file: FileEntry) {
  if (file.is_dir) {
    emit('navigate', file.path);
  }
}

function onDblClickParent() {
  emit('navigate-up');
}

function onListKeydown(e: KeyboardEvent) {
  const mod = e.metaKey || e.ctrlKey;
  if (mod && e.key.toLowerCase() === 'a') {
    e.preventDefault();
    emit('update:selected', props.files.map((f) => f.path));
  } else if (e.key === 'Delete' || (mod && e.key === 'Backspace')) {
    e.preventDefault();
    emit('delete');
  } else if (e.key === 'F2') {
    e.preventDefault();
    emit('rename');
  } else if (e.key === 'F5') {
    e.preventDefault();
    emit('transfer');
  } else if (e.key === 'F7') {
    e.preventDefault();
    emit('mkdir');
  } else if (e.key === 'Enter' && props.selected.length === 1) {
    const file = props.files.find((f) => f.path === props.selected[0]);
    if (file) onDblClick(file);
  } else if (e.key === 'Backspace') {
    e.preventDefault();
    emit('navigate-up');
  }
}

function onDragStart(e: DragEvent, file: FileEntry) {
  // Dragging an unselected row drags just that row.
  const paths = selectedSet.value.has(file.path) ? props.selected : [file.path];
  if (!selectedSet.value.has(file.path)) emit('update:selected', paths);
  e.dataTransfer?.setData(DRAG_TYPE, JSON.stringify({ from: props.side, paths }));
  if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy';
}

function isForeignDrag(e: DragEvent) {
  return e.dataTransfer?.types.includes(DRAG_TYPE) ?? false;
}

function onDragOver(e: DragEvent) {
  if (!isForeignDrag(e)) return;
  e.preventDefault();
  if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
  dropTarget.value = true;
}

function onDrop(e: DragEvent) {
  dropTarget.value = false;
  const raw = e.dataTransfer?.getData(DRAG_TYPE);
  if (!raw) return;
  e.preventDefault();
  const { from, paths } = JSON.parse(raw) as { from: PaneSide; paths: string[] };
  if (from !== props.side) emit('drop', from, paths);
}

function formatSize(bytes: number): string {
  if (bytes === 0) return '—';
  const units = ['o', 'Ko', 'Mo', 'Go'];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return (bytes / Math.pow(1024, i)).toFixed(i > 0 ? 1 : 0) + ' ' + units[i];
}

function formatDate(timestamp?: number): string {
  if (!timestamp) return '—';
  return new Date(timestamp * 1000).toLocaleDateString('fr-FR', {
    day: '2-digit',
    month: '2-digit',
    year: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}
</script>

<template>
  <div
    :class="['file-pane', { 'pane-active': active, 'drop-target': dropTarget }]"
    @mousedown="emit('activate')"
    @dragover="onDragOver"
    @dragleave="dropTarget = false"
    @drop="onDrop"
  >
    <div class="pane-header">
      <span class="pane-title">{{ props.title }}</span>
    </div>

    <div class="path-bar">
      <input
        type="text"
        class="path-input"
        v-model="editPath"
        @keydown="onPathKeydown"
      />
    </div>

    <div class="file-list-container" tabindex="0" @keydown="onListKeydown" @focus="emit('activate')">
      <div v-if="props.loading" class="loading-indicator">Chargement...</div>
      <table v-else class="file-list">
        <thead>
          <tr>
            <th class="col-name">Nom</th>
            <th class="col-size">Taille</th>
            <th class="col-date">Modifié</th>
            <th class="col-perms">Droits</th>
          </tr>
        </thead>
        <tbody>
          <tr class="file-row file-row-parent" @dblclick="onDblClickParent">
            <td class="col-name">
              <span class="file-icon">📁</span>
              ..
            </td>
            <td class="col-size"></td>
            <td class="col-date"></td>
            <td class="col-perms"></td>
          </tr>
          <tr
            v-for="(file, index) in props.files"
            :key="file.path"
            :class="['file-row', { selected: selectedSet.has(file.path) }]"
            draggable="true"
            @click="onRowClick($event, file, index)"
            @dblclick="onDblClick(file)"
            @dragstart="onDragStart($event, file)"
          >
            <td class="col-name">
              <span class="file-icon">{{ file.is_dir ? '📁' : '📄' }}</span>
              {{ file.name }}
            </td>
            <td class="col-size">{{ file.is_dir ? '' : formatSize(file.size) }}</td>
            <td class="col-date">{{ formatDate(file.modified) }}</td>
            <td class="col-perms">{{ file.permissions ?? '' }}</td>
          </tr>
        </tbody>
      </table>
    </div>
  </div>
</template>

<style scoped>
.file-pane {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--color-bg);
}

.pane-header {
  padding: 4px 8px;
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
}

.pane-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.path-bar {
  padding: 4px;
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
}

.path-input {
  width: 100%;
  padding: 3px 6px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 12px;
  font-family: var(--font-mono);
  box-sizing: border-box;
}

.file-list-container {
  flex: 1;
  overflow-y: auto;
}

.loading-indicator {
  padding: 16px;
  text-align: center;
  color: var(--color-text-muted);
  font-size: 12px;
}

.file-list {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
}

.file-list thead {
  position: sticky;
  top: 0;
  background: var(--color-surface);
}

.file-list th {
  padding: 4px 8px;
  text-align: left;
  font-weight: 600;
  color: var(--color-text-muted);
  border-bottom: 1px solid var(--color-border);
  font-size: 11px;
  text-transform: uppercase;
  letter-spacing: 0.3px;
  user-select: none;
}

.file-row {
  cursor: pointer;
}

.file-row:hover {
  background: var(--color-hover);
}

.file-row.selected {
  background: var(--color-active);
}

.pane-active .file-row.selected {
  background: #094771;
}

.pane-active .pane-title {
  color: var(--color-text);
}

.drop-target .file-list-container {
  outline: 2px dashed var(--color-accent);
  outline-offset: -2px;
}

.file-list-container:focus {
  outline: none;
}

.file-row {
  user-select: none;
}

.file-row td {
  padding: 3px 8px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  color: var(--color-text);
}

.col-name {
  min-width: 150px;
}

.col-size {
  width: 80px;
  text-align: right;
}

.col-date {
  width: 130px;
}

.col-perms {
  width: 80px;
  font-family: var(--font-mono);
}

.file-icon {
  margin-right: 4px;
  font-size: 13px;
}
</style>
