import { ref, computed } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { ConflictStrategy, FileEntry, PaneSide } from '../types';
import { useLogStore } from './logs';
import { useLocalFilesStore } from './localFiles';
import { useConnectionStore } from './connection';
import { useTransfersStore } from './transfers';
import { useSettingsStore } from './settings';
import { useEditsStore } from './edits';
import { errorMessage } from '../utils/errors';
import { t } from '../i18n';

export type FileDialog =
  | { type: 'mkdir'; side: PaneSide }
  | { type: 'rename'; side: PaneSide; entry: FileEntry }
  | { type: 'delete'; side: PaneSide; entries: FileEntry[] }
  | { type: 'properties'; side: PaneSide; entries: FileEntry[] }
  | { type: 'move'; side: PaneSide; entries: FileEntry[]; destDir: string }
  | { type: 'conflict'; name: string; remaining: number };

export type ConflictChoice = Exclude<ConflictStrategy, 'Ask' | 'Merge'> | 'Cancel';

export const useFileActionsStore = defineStore('fileActions', () => {
  const activePane = ref<PaneSide>('local');
  const dialog = ref<FileDialog | null>(null);
  // Resolves the pending conflict question; kept out of reactive state.
  let resolveConflict: ((choice: { choice: ConflictChoice; applyToAll: boolean }) => void) | null =
    null;

  function pane(side: PaneSide) {
    return side === 'local' ? useLocalFilesStore() : useConnectionStore();
  }

  function selectionOf(side: PaneSide): FileEntry[] {
    const store = pane(side);
    const chosen = new Set(store.selected);
    return store.files.filter((f) => chosen.has(f.path));
  }

  const connected = computed(() => useConnectionStore().connected);
  const activeSelection = computed(() => selectionOf(activePane.value));
  const paneReady = computed(() => activePane.value === 'local' || connected.value);

  const canMkdir = computed(() => paneReady.value && !!pane(activePane.value).currentPath);
  const canRename = computed(() => paneReady.value && activeSelection.value.length === 1);
  const canDelete = computed(() => paneReady.value && activeSelection.value.length > 0);
  const canShowProperties = canDelete;
  const canUpload = computed(() => connected.value && selectionOf('local').length > 0);
  const canDownload = computed(() => connected.value && selectionOf('remote').length > 0);
  const canMove = computed(() => connected.value && activeSelection.value.length > 0);
  const canEdit = computed(
    () =>
      connected.value &&
      activePane.value === 'remote' &&
      activeSelection.value.length === 1 &&
      !activeSelection.value[0].is_dir,
  );

  /// Move = copy to the other pane, then remove the source; asked first.
  function requestMove(side: PaneSide = activePane.value) {
    const entries = selectionOf(side);
    if (!connected.value || entries.length === 0) return;
    const destDir = pane(side === 'local' ? 'remote' : 'local').currentPath;
    dialog.value = { type: 'move', side, entries, destDir };
  }

  function requestEdit() {
    if (canEdit.value) useEditsStore().open(activeSelection.value[0]);
  }

  function requestMkdir() {
    if (canMkdir.value) dialog.value = { type: 'mkdir', side: activePane.value };
  }

  function requestRename() {
    if (canRename.value) {
      dialog.value = { type: 'rename', side: activePane.value, entry: activeSelection.value[0] };
    }
  }

  function requestDelete() {
    if (canDelete.value) {
      dialog.value = { type: 'delete', side: activePane.value, entries: activeSelection.value };
    }
  }

  function requestProperties() {
    if (canShowProperties.value) {
      dialog.value = { type: 'properties', side: activePane.value, entries: activeSelection.value };
    }
  }

  /// Applies `mode` to one entry; returns whether it worked. The pane is refreshed either way.
  async function setPermissions(side: PaneSide, entry: FileEntry, mode: number): Promise<boolean> {
    const store = pane(side);
    const command = side === 'local' ? 'set_local_permissions' : 'set_remote_permissions';
    const octal = mode.toString(8).padStart(4, '0');
    try {
      await invoke(command, { path: entry.path, mode });
      useLogStore().addLog('Info', t.log.permissions, t.log.ok, `${entry.path} → ${octal}`);
      return true;
    } catch (err) {
      useLogStore().addLog('Error', t.log.permissions, t.log.failed, `${entry.path} — ${errorMessage(err)}`);
      await useConnectionStore().handleConnectionLost(err);
      return false;
    } finally {
      if (side === 'local' || useConnectionStore().connected) await store.refresh();
    }
  }

  function closeDialog() {
    dialog.value = null;
  }

  async function mkdir(side: PaneSide, name: string) {
    closeDialog();
    const store = pane(side);
    const command = side === 'local' ? 'create_local_dir' : 'create_remote_dir';
    try {
      const path = await invoke<string>(command, { parent: store.currentPath, name });
      useLogStore().addLog('Info', t.log.mkdir, t.log.ok, path);
      await store.refresh();
    } catch (err) {
      useLogStore().addLog('Error', t.log.mkdir, t.log.failed, `${name} — ${errorMessage(err)}`);
      await useConnectionStore().handleConnectionLost(err);
    }
  }

  async function rename(side: PaneSide, entry: FileEntry, newName: string) {
    closeDialog();
    if (newName === entry.name) return;
    const store = pane(side);
    const command = side === 'local' ? 'rename_local' : 'rename_remote';
    try {
      const target = await invoke<string>(command, { path: entry.path, newName });
      useLogStore().addLog('Info', t.log.rename, t.log.ok, `${entry.path} → ${target}`);
      await store.refresh();
      store.selected = [target];
    } catch (err) {
      useLogStore().addLog('Error', t.log.rename, t.log.failed, `${entry.path} — ${errorMessage(err)}`);
      await useConnectionStore().handleConnectionLost(err);
    }
  }

  async function remove(side: PaneSide, entries: FileEntry[]) {
    closeDialog();
    const store = pane(side);
    const command = side === 'local' ? 'delete_local' : 'delete_remote';
    for (const entry of entries) {
      try {
        await invoke(command, { path: entry.path });
        useLogStore().addLog('Info', t.log.delete, t.log.ok, entry.path);
      } catch (err) {
        useLogStore().addLog('Error', t.log.delete, t.log.failed, `${entry.path} — ${errorMessage(err)}`);
        await useConnectionStore().handleConnectionLost(err);
        if (!useConnectionStore().connected && side === 'remote') return;
      }
    }
    await store.refresh();
  }

  function askConflict(name: string, remaining: number) {
    dialog.value = { type: 'conflict', name, remaining };
    return new Promise<{ choice: ConflictChoice; applyToAll: boolean }>((resolve) => {
      resolveConflict = resolve;
    });
  }

  function answerConflict(choice: ConflictChoice, applyToAll: boolean) {
    closeDialog();
    resolveConflict?.({ choice, applyToAll });
    resolveConflict = null;
  }

  /// Queues the selection of `from` into the current directory of the other pane,
  /// asking what to do for names that already exist there.
  /// `entries` may also be files dropped from outside the app, known only by path and name.
  async function transfer(
    from: PaneSide,
    entries: Pick<FileEntry, 'path' | 'name'>[] = selectionOf(from),
    move = false,
  ) {
    if (!connected.value || entries.length === 0) return;
    const to: PaneSide = from === 'local' ? 'remote' : 'local';
    const destDir = pane(to).currentPath;
    const existing = new Set(pane(to).files.map((f) => f.name));
    const direction = from === 'local' ? 'Upload' : 'Download';
    const transfers = useTransfersStore();

    const preset = useSettingsStore().settings.default_conflict;
    let remembered: ConflictStrategy | null = preset === 'Ask' ? null : preset;
    const conflicts = entries.filter((e) => existing.has(e.name)).length;
    let seen = 0;

    for (const entry of entries) {
      let strategy: ConflictStrategy = 'Skip';
      if (existing.has(entry.name)) {
        seen += 1;
        if (remembered) {
          strategy = remembered;
        } else {
          const answer = await askConflict(entry.name, conflicts - seen);
          if (answer.choice === 'Cancel') return;
          strategy = answer.choice;
          if (answer.applyToAll) remembered = strategy;
        }
      }
      // Skip still goes to the backend: it re-checks, since the listing may be stale.
      await transfers.enqueue(direction, entry.path, destDir, strategy, undefined, move);
    }
  }

  return {
    activePane,
    dialog,
    activeSelection,
    canMkdir,
    canRename,
    canDelete,
    canUpload,
    canDownload,
    requestMkdir,
    requestRename,
    requestDelete,
    canShowProperties,
    requestProperties,
    setPermissions,
    closeDialog,
    mkdir,
    rename,
    remove,
    answerConflict,
    transfer,
    canMove,
    requestMove,
    canEdit,
    requestEdit,
  };
});
