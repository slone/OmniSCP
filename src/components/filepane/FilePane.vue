<script lang="ts">
import { reactive } from 'vue';

export type SortKey = 'name' | 'size' | 'modified';

/// Kept per side outside the component, so the remote pane keeps its order across connections.
const sortBySide = reactive<Record<string, { key: SortKey; ascending: boolean }>>({
  local: { key: 'name', ascending: true },
  remote: { key: 'name', ascending: true },
});

const nameCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

export type ColumnKey = 'name' | 'size' | 'modified' | 'permissions' | 'owner';
type Widths = Partial<Record<ColumnKey, number>>;

/// The class each column's cells carry.
const COLUMN_CLASS: Record<ColumnKey, string> = {
  name: 'col-name',
  size: 'col-size',
  modified: 'col-date',
  permissions: 'col-perms',
  owner: 'col-owner',
};
/// The name column takes the room left until the user sets its width.
const DEFAULT_WIDTHS: Widths = { size: 80, modified: 120, permissions: 80, owner: 105 };
const MIN_WIDTH = 40;
const WIDTHS_KEY = 'omniscp.columnWidths';

function loadWidths(): Record<string, Widths> {
  try {
    const saved = JSON.parse(localStorage.getItem(WIDTHS_KEY) ?? '{}');
    return { local: saved.local ?? {}, remote: saved.remote ?? {} };
  } catch {
    return { local: {}, remote: {} };
  }
}

/// Column widths set by the user, per side; kept in this browser's storage.
const widthsBySide = reactive<Record<string, Widths>>(loadWidths());

function saveWidths() {
  try {
    localStorage.setItem(WIDTHS_KEY, JSON.stringify(widthsBySide));
  } catch {
    // Not worth more than losing the widths at the next start.
  }
}
</script>

<script setup lang="ts">
import { ref, watch, computed, nextTick, useId } from 'vue';
import type { FileEntry, PaneSide } from '../../types';
import { formatDate, formatSize } from '../../utils/format';
import { useSettingsStore } from '../../stores/settings';
import { t } from '../../i18n';
import EntryIcon from '../dialogs/EntryIcon.vue';
import { trackPress } from '../../utils/paneDrag';

const props = defineProps<{
  title: string;
  side: PaneSide;
  path: string;
  files: FileEntry[];
  selected: string[];
  active: boolean;
  loading?: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  /// Why the last navigation failed; the previous listing stays shown.
  error?: string | null;
  /// Something dragged over this pane would land here.
  dropHighlight?: boolean;
}>();

const emit = defineEmits<{
  navigate: [path: string];
  'navigate-up': [];
  back: [];
  forward: [];
  refresh: [];
  properties: [];
  'update:selected': [paths: string[]];
  activate: [];
  mkdir: [];
  rename: [];
  delete: [];
  transfer: [];
  /// Copy to the other pane, then remove the source.
  move: [];
  /// Open in a local application (remote files).
  edit: [];
  /// Files dragged from this pane were dropped on the other one.
  drop: [from: PaneSide, paths: string[]];
}>();

const editPath = ref(props.path);
const anchor = ref<string | null>(null);
/// The row the keyboard is on; screen readers announce it.
const cursor = ref<string | null>(null);
const rowIdPrefix = useId();

const filterText = ref('');
const settings = useSettingsStore();
const sort = computed(() => sortBySide[props.side]);

const selectedSet = computed(() => new Set(props.selected));

const columnKeys = Object.keys(COLUMN_CLASS) as ColumnKey[];
const widths = computed<Widths>(() => ({ ...DEFAULT_WIDTHS, ...widthsBySide[props.side] }));
const table = ref<HTMLTableElement | null>(null);

function columnStyle(key: ColumnKey) {
  const width = widths.value[key];
  return width ? { width: `${width}px` } : undefined;
}

/// Fixed widths add up; until the name column has one, it fills the pane.
const tableStyle = computed(() => {
  const others = columnKeys.filter((k) => k !== 'name').reduce((sum, k) => sum + (widths.value[k] ?? 0), 0);
  const name = widths.value.name;
  return name ? { width: `${name + others}px` } : { width: '100%', minWidth: `${others + 150}px` };
});

function setWidth(key: ColumnKey, width: number) {
  widthsBySide[props.side] = { ...widthsBySide[props.side], [key]: Math.max(MIN_WIDTH, Math.round(width)) };
}

/// Dragging the border at the right of a header.
function startResize(e: MouseEvent, key: ColumnKey) {
  const header = (e.currentTarget as HTMLElement).parentElement;
  if (!header) return;
  const startX = e.clientX;
  const startWidth = header.getBoundingClientRect().width;
  const move = (m: MouseEvent) => setWidth(key, startWidth + m.clientX - startX);
  const up = () => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    document.documentElement.classList.remove('col-resizing');
    saveWidths();
  };
  document.documentElement.classList.add('col-resizing');
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
}

/// Double-clicking the border fits the column to its widest content on screen.
async function autoSize(key: ColumnKey) {
  // Narrowed first, each cell's scroll width is its content's own width.
  setWidth(key, MIN_WIDTH);
  await nextTick();
  const cells = table.value?.querySelectorAll<HTMLElement>(`.${COLUMN_CLASS[key]}`) ?? [];
  const widest = Math.max(MIN_WIDTH, ...Array.from(cells, (cell) => cell.scrollWidth));
  setWidth(key, widest + 2);
  saveWidths();
}

function compare(a: FileEntry, b: FileEntry): number {
  // Folders stay above files whatever the order, as in most file managers.
  if (a.is_dir !== b.is_dir) return a.is_dir ? -1 : 1;
  let result = 0;
  if (sort.value.key === 'size') result = a.size - b.size;
  else if (sort.value.key === 'modified') result = (a.modified ?? 0) - (b.modified ?? 0);
  if (result === 0) result = nameCollator.compare(a.name, b.name);
  return sort.value.ascending ? result : -result;
}

/// What the list shows, in order. Selection, ranges and Ctrl+A work on this, never on hidden rows.
const visibleFiles = computed(() => {
  const needle = filterText.value.trim().toLowerCase();
  return props.files
    .filter((f) => settings.settings.show_hidden_files || !f.name.startsWith('.'))
    .filter((f) => !needle || f.name.toLowerCase().includes(needle))
    .sort(compare);
});

const hiddenCount = computed(() => props.files.length - visibleFiles.value.length);

// A row that disappears from view must not stay selected: it would be deleted or sent unseen.
watch(visibleFiles, (files) => {
  const visible = new Set(files.map((f) => f.path));
  const kept = props.selected.filter((p) => visible.has(p));
  if (kept.length !== props.selected.length) emit('update:selected', kept);
});

function sortBy(key: SortKey) {
  const current = sortBySide[props.side];
  sortBySide[props.side] =
    current.key === key ? { key, ascending: !current.ascending } : { key, ascending: true };
}

function sortMark(key: SortKey) {
  if (sort.value.key !== key) return '';
  return sort.value.ascending ? ' ▲' : ' ▼';
}

function ariaSort(key: SortKey) {
  if (sort.value.key !== key) return 'none';
  return sort.value.ascending ? 'ascending' : 'descending';
}

function ownerTitle(file: FileEntry) {
  const user = file.owner ?? file.uid;
  const group = file.group ?? file.gid;
  return user === undefined ? '' : t.panes.ownerTitle(String(user), group === undefined ? undefined : String(group));
}

function onFilterKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') filterText.value = '';
}

watch(() => props.path, (val) => {
  editPath.value = val;
  anchor.value = null;
  cursor.value = null;
  filterText.value = '';
});

function onPathKeydown(e: KeyboardEvent) {
  if (e.key === 'Enter') {
    emit('navigate', editPath.value);
  } else if (e.key === 'Escape') {
    editPath.value = props.path;
  }
}

function onRowClick(e: MouseEvent, file: FileEntry, index: number) {
  emit('activate');
  if (e.shiftKey && anchor.value) {
    const from = visibleFiles.value.findIndex((f) => f.path === anchor.value);
    if (from >= 0) {
      const [lo, hi] = from < index ? [from, index] : [index, from];
      emit('update:selected', visibleFiles.value.slice(lo, hi + 1).map((f) => f.path));
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
  cursor.value = file.path;
}

const cursorIndex = computed(() => visibleFiles.value.findIndex((f) => f.path === cursor.value));

function rowId(index: number) {
  return `${rowIdPrefix}-${index}`;
}

/// Arrow keys, Home and End move the cursor; Shift extends the selection from the anchor.
function moveCursor(index: number, extend: boolean) {
  const files = visibleFiles.value;
  if (files.length === 0) return;
  const target = files[Math.max(0, Math.min(files.length - 1, index))];
  cursor.value = target.path;
  const from = files.findIndex((f) => f.path === anchor.value);
  if (extend && from >= 0) {
    const to = files.indexOf(target);
    const [lo, hi] = from < to ? [from, to] : [to, from];
    emit('update:selected', files.slice(lo, hi + 1).map((f) => f.path));
  } else {
    anchor.value = target.path;
    emit('update:selected', [target.path]);
  }
  nextTick(() => document.getElementById(rowId(files.indexOf(target)))?.scrollIntoView({ block: 'nearest' }));
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
  const plain = !mod && !e.altKey;
  const current = cursorIndex.value >= 0 ? cursorIndex.value : visibleFiles.value.findIndex((f) => selectedSet.value.has(f.path));
  if (plain && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
    e.preventDefault();
    const step = e.key === 'ArrowDown' ? 1 : -1;
    moveCursor(current < 0 ? (step > 0 ? 0 : visibleFiles.value.length - 1) : current + step, e.shiftKey);
  } else if (plain && (e.key === 'Home' || e.key === 'End')) {
    e.preventDefault();
    moveCursor(e.key === 'Home' ? 0 : visibleFiles.value.length - 1, e.shiftKey);
  } else if (mod && e.key.toLowerCase() === 'a') {
    e.preventDefault();
    emit('update:selected', visibleFiles.value.map((f) => f.path));
  } else if (e.key === 'Delete' || (mod && e.key === 'Backspace')) {
    e.preventDefault();
    emit('delete');
  } else if (e.key === 'F2') {
    e.preventDefault();
    emit('rename');
  } else if (e.key === 'F5') {
    e.preventDefault();
    emit('transfer');
  } else if (e.key === 'F6') {
    e.preventDefault();
    emit('move');
  } else if (e.key === 'F4') {
    e.preventDefault();
    emit('edit');
  } else if (e.key === 'F7') {
    e.preventDefault();
    emit('mkdir');
  } else if (e.key === 'Enter' && props.selected.length === 1) {
    const file = visibleFiles.value.find((f) => f.path === props.selected[0]);
    if (file) onDblClick(file);
  } else if ((e.altKey && e.key === 'ArrowLeft') || (mod && e.key === '[')) {
    e.preventDefault();
    emit('back');
  } else if ((e.altKey && e.key === 'ArrowRight') || (mod && e.key === ']')) {
    e.preventDefault();
    emit('forward');
  } else if ((e.altKey || mod) && e.key === 'ArrowUp') {
    e.preventDefault();
    emit('navigate-up');
  } else if (mod && e.key.toLowerCase() === 'r') {
    // Also keeps the webview from reloading the whole app.
    e.preventDefault();
    emit('refresh');
  } else if ((mod && e.key.toLowerCase() === 'i') || (e.altKey && e.key === 'Enter')) {
    e.preventDefault();
    emit('properties');
  } else if (e.key === 'Backspace') {
    e.preventDefault();
    emit('navigate-up');
  }
}

/// The back and forward buttons of a mouse.
function onMouseUp(e: MouseEvent) {
  if (e.button === 3) emit('back');
  else if (e.button === 4) emit('forward');
}

function onRowMouseDown(e: MouseEvent, file: FileEntry) {
  if (e.button !== 0) return;
  // A press on a row selects rows, never text: otherwise dragging to the
  // other pane would select every text in between. The list still takes focus.
  e.preventDefault();
  (e.currentTarget as HTMLElement).closest<HTMLElement>('.file-list-container')?.focus();
  trackPress(
    e,
    () => {
      // Dragging an unselected row drags just that row.
      const paths = selectedSet.value.has(file.path) ? props.selected : [file.path];
      if (!selectedSet.value.has(file.path)) emit('update:selected', paths);
      return { from: props.side, paths, label: paths.length === 1 ? file.name : t.common.items(paths.length) };
    },
    (drag) => emit('drop', drag.from, drag.paths),
  );
}

</script>

<template>
  <div
    :class="['file-pane', { 'pane-active': active, 'drop-target': dropHighlight }]"
    :data-pane="side"
    @mousedown="emit('activate')"
    @mouseup="onMouseUp"
  >
    <div class="pane-header">
      <span class="pane-title">{{ props.title }}</span>
      <span v-if="hiddenCount > 0" class="hidden-count" :title="t.panes.hiddenHint">
        {{ t.panes.hidden(hiddenCount) }}
      </span>
      <input
        v-model="filterText"
        type="search"
        class="filter-input"
        :placeholder="t.panes.filter"
        :aria-label="t.panes.filterLabel"
        spellcheck="false"
        @keydown="onFilterKeydown"
      />
    </div>

    <div class="path-bar">
      <button class="nav-btn" :title="t.panes.backHint" :aria-label="t.panes.back" :disabled="!canGoBack" @click="emit('back')">←</button>
      <button class="nav-btn" :title="t.panes.forwardHint" :aria-label="t.panes.forward" :disabled="!canGoForward" @click="emit('forward')">→</button>
      <button class="nav-btn" :title="t.panes.upHint" :aria-label="t.panes.up" @click="emit('navigate-up')">↑</button>
      <button class="nav-btn" :title="t.panes.refreshHint" :aria-label="t.panes.refresh" @click="emit('refresh')">↻</button>
      <input
        type="text"
        class="path-input"
        :aria-label="t.panes.path"
        spellcheck="false"
        v-model="editPath"
        @keydown="onPathKeydown"
      />
    </div>

    <div v-if="error" class="pane-error" role="alert">{{ error }}</div>

    <div
      class="file-list-container"
      tabindex="0"
      role="grid"
      aria-multiselectable="true"
      :aria-label="t.panes.listLabel(props.title)"
      :aria-busy="props.loading"
      :aria-activedescendant="cursorIndex >= 0 ? rowId(cursorIndex) : undefined"
      @keydown="onListKeydown"
      @focus="emit('activate')"
    >
      <div v-if="props.loading" class="loading-indicator" role="status">{{ t.panes.loading }}</div>
      <table v-else ref="table" class="file-list" role="presentation" :style="tableStyle">
        <colgroup>
          <col v-for="key in columnKeys" :key="key" :style="columnStyle(key)" />
        </colgroup>
        <thead role="rowgroup">
          <tr role="row">
            <th role="columnheader" class="col-name" :aria-sort="ariaSort('name')">
              <button class="sort-btn" tabindex="-1" @click="sortBy('name')">{{ t.panes.name }}{{ sortMark('name') }}</button>
              <span class="col-resizer" :title="t.panes.resizeHint" aria-hidden="true" @mousedown.stop.prevent="startResize($event, 'name')" @dblclick.stop="autoSize('name')" />
            </th>
            <th role="columnheader" class="col-size" :aria-sort="ariaSort('size')">
              <button class="sort-btn" tabindex="-1" @click="sortBy('size')">{{ t.panes.size }}{{ sortMark('size') }}</button>
              <span class="col-resizer" :title="t.panes.resizeHint" aria-hidden="true" @mousedown.stop.prevent="startResize($event, 'size')" @dblclick.stop="autoSize('size')" />
            </th>
            <th role="columnheader" class="col-date" :aria-sort="ariaSort('modified')">
              <button class="sort-btn" tabindex="-1" @click="sortBy('modified')">{{ t.panes.modified }}{{ sortMark('modified') }}</button>
              <span class="col-resizer" :title="t.panes.resizeHint" aria-hidden="true" @mousedown.stop.prevent="startResize($event, 'modified')" @dblclick.stop="autoSize('modified')" />
            </th>
            <th role="columnheader" class="col-perms">
              {{ t.panes.permissions }}
              <span class="col-resizer" :title="t.panes.resizeHint" aria-hidden="true" @mousedown.stop.prevent="startResize($event, 'permissions')" @dblclick.stop="autoSize('permissions')" />
            </th>
            <th role="columnheader" class="col-owner">
              {{ t.panes.owner }}
              <span class="col-resizer" :title="t.panes.resizeHint" aria-hidden="true" @mousedown.stop.prevent="startResize($event, 'owner')" @dblclick.stop="autoSize('owner')" />
            </th>
          </tr>
        </thead>
        <tbody role="rowgroup">
          <tr class="file-row file-row-parent" role="row" :aria-label="t.panes.parent" @dblclick="onDblClickParent">
            <td class="col-name" role="gridcell">
              <span class="file-icon" aria-hidden="true">📁</span>
              ..
            </td>
            <td class="col-size" role="gridcell"></td>
            <td class="col-date" role="gridcell"></td>
            <td class="col-perms" role="gridcell"></td>
            <td class="col-owner" role="gridcell"></td>
          </tr>
          <tr
            v-for="(file, index) in visibleFiles"
            :id="rowId(index)"
            :key="file.path"
            :class="['file-row', { selected: selectedSet.has(file.path), cursor: cursor === file.path }]"
            role="row"
            :aria-selected="selectedSet.has(file.path)"
            @mousedown="onRowMouseDown($event, file)"
            @click="onRowClick($event, file, index)"
            @dblclick="onDblClick(file)"
          >
            <td class="col-name" role="gridcell">
              <EntryIcon class="file-icon" :entry="file" />
              {{ file.name }}
            </td>
            <td class="col-size" role="gridcell">{{ file.is_dir ? '' : formatSize(file.size) }}</td>
            <td class="col-date" role="gridcell">{{ formatDate(file.modified) }}</td>
            <td class="col-perms" role="gridcell">{{ file.permissions ?? '' }}</td>
            <td class="col-owner" role="gridcell" :title="ownerTitle(file)">{{ file.owner ?? file.uid ?? '' }}</td>
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
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 8px;
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
}

.hidden-count {
  margin-left: auto;
  font-size: 11px;
  color: var(--color-text-muted);
}

.filter-input {
  width: 120px;
  margin-left: auto;
  padding: 1px 6px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 11px;
}

.hidden-count + .filter-input {
  margin-left: 0;
}

.sort-btn {
  padding: 0;
  border: none;
  background: none;
  color: inherit;
  font: inherit;
  text-align: inherit;
  text-transform: inherit;
  letter-spacing: inherit;
  cursor: pointer;
}

.sort-btn:hover {
  color: var(--color-text);
}

.pane-title {
  font-size: 12px;
  font-weight: 600;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.5px;
}

.path-bar {
  display: flex;
  gap: 2px;
  padding: 4px;
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
}

.pane-error {
  padding: 4px 8px;
  border-bottom: 1px solid var(--color-danger-border);
  background: var(--bg-danger);
  color: var(--color-danger-text);
  font-size: 12px;
  word-break: break-word;
}

.nav-btn {
  width: 24px;
  flex-shrink: 0;
  border: 1px solid transparent;
  border-radius: 3px;
  background: none;
  color: var(--color-text);
  font-size: 13px;
  cursor: pointer;
}

.nav-btn:hover:not(:disabled) {
  background: var(--color-hover);
  border-color: var(--color-border);
}

.nav-btn:disabled {
  opacity: 0.35;
  cursor: default;
}

.path-input {
  flex: 1;
  min-width: 0;
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
  overflow: auto;
}

.loading-indicator {
  padding: 16px;
  text-align: center;
  color: var(--color-text-muted);
  font-size: 12px;
}

.file-list {
  table-layout: fixed;
  border-collapse: collapse;
  font-size: 12px;
}

.file-list thead {
  position: sticky;
  top: 0;
  background: var(--color-surface);
}

.file-list th {
  position: relative;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
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
  background: var(--color-selected);
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

.file-list-container:focus-visible .file-row.cursor td {
  box-shadow: inset 0 1px 0 var(--color-accent), inset 0 -1px 0 var(--color-accent);
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

.col-size {
  text-align: right;
}

.col-perms {
  font-family: var(--font-mono);
}

/* The border at the right of a header: drag to resize, double-click to fit. */
.col-resizer {
  position: absolute;
  top: 0;
  right: 0;
  width: 6px;
  height: 100%;
  cursor: col-resize;
}

.col-resizer:hover {
  background: var(--color-border);
}

.file-icon {
  margin-right: 4px;
  font-size: 13px;
}
</style>
