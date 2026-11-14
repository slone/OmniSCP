import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { EditInfo, FileEntry } from '../types';
import { useLogStore } from './logs';
import { useConnectionStore } from './connection';
import { errorMessage, isBackendError } from '../utils/errors';
import { t } from '../i18n';

export interface EditedFile extends EditInfo {
  /// conflict: the server's copy changed since it was opened; the save waits for the user.
  state: 'watching' | 'sending' | 'conflict' | 'error';
  error?: string;
  /// When the last save reached the server.
  sentAt?: number;
  /// Saved again while sending: send once more afterwards.
  again?: boolean;
}

function parentOf(path: string) {
  return path.replace(/\/[^/]+$/, '') || '/';
}

export const useEditsStore = defineStore('edits', () => {
  const files = ref<EditedFile[]>([]);
  /// Bumped when something needs the user's attention, so the panel can show it.
  const attention = ref(0);
  let listening = false;

  async function ensureListener() {
    if (listening) return;
    listening = true;
    await listen<{ id: string }>('remote-edit-changed', (event) => send(event.payload.id, false));
  }

  /// Downloads the file and opens it in the system's default application.
  async function open(entry: FileEntry) {
    await ensureListener();
    const log = useLogStore();
    try {
      const info = await invoke<EditInfo>('edit_remote_file', { path: entry.path });
      if (!files.value.some((f) => f.id === info.id)) {
        files.value.push({ ...info, state: 'watching' });
        log.addLog('Info', t.log.edit, t.log.editOpened, t.log.editLocalCopy(info.remote_path, info.local_path));
      }
    } catch (err) {
      log.addLog('Error', t.log.edit, t.log.failed, `${entry.path} — ${errorMessage(err)}`);
      await useConnectionStore().handleConnectionLost(err);
    }
  }

  /// `force`: send even though the server's copy changed since it was opened.
  async function send(id: string, force: boolean) {
    const file = files.value.find((f) => f.id === id);
    if (!file) return;
    if (file.state === 'sending') {
      file.again = true;
      return;
    }
    // A conflict waits for the user; another save does not decide for them.
    if (file.state === 'conflict' && !force) return;
    const log = useLogStore();
    file.state = 'sending';
    file.error = undefined;
    try {
      await invoke('upload_edited_file', { id, force });
      file.state = 'watching';
      file.sentAt = Date.now();
      log.addLog('Info', t.log.edit, t.log.editSent, file.remote_path);
      const connection = useConnectionStore();
      if (connection.currentPath === parentOf(file.remote_path)) await connection.refresh();
      if (file.again) {
        file.again = false;
        await send(id, false);
      }
    } catch (err) {
      const message = errorMessage(err);
      file.error = message;
      if (isBackendError(err) && err.kind === 'Conflict') {
        file.state = 'conflict';
        log.addLog('Warning', t.log.edit, t.log.editConflict, `${message} — ${t.log.editNotSent}`);
      } else {
        file.state = 'error';
        log.addLog('Error', t.log.edit, t.log.failed, `${file.remote_path} — ${message}`);
        await useConnectionStore().handleConnectionLost(err);
      }
      attention.value++;
    }
  }

  /// Keeps the local change unsent; the next save is checked again.
  function dismiss(id: string) {
    const file = files.value.find((f) => f.id === id);
    if (file) {
      file.state = 'watching';
      file.error = undefined;
    }
  }

  async function stop(id: string) {
    await invoke('stop_editing', { id }).catch(() => {});
    const file = files.value.find((f) => f.id === id);
    files.value = files.value.filter((f) => f.id !== id);
    if (file) useLogStore().addLog('Info', t.log.edit, t.log.editStopped, file.remote_path);
  }

  /// The backend forgets the edits with the connection; local copies stay on disk.
  function reset() {
    if (files.value.length > 0) {
      useLogStore().addLog('Info', t.log.edit, t.log.editStopped, t.log.editsForgotten(files.value.length));
    }
    files.value = [];
  }

  return { files, attention, open, send, dismiss, stop, reset };
});
