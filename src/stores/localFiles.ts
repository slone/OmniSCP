import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { FileEntry } from '../types';
import { useLogStore } from './logs';
import { useNavHistory, type NavKind } from '../utils/history';
import { errorMessage } from '../utils/errors';
import { t } from '../i18n';

export const useLocalFilesStore = defineStore('localFiles', () => {
  const currentPath = ref('');
  const files = ref<FileEntry[]>([]);
  const selected = ref<string[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);
  const history = useNavHistory();

  async function initialize() {
    try {
      const home = await invoke<string>('get_home_dir');
      await navigateTo(home);
    } catch (err) {
      const logStore = useLogStore();
      const message = errorMessage(err);
      logStore.addLog('Error', t.log.startup, t.log.failed, message);
      error.value = message;
    }
  }

  async function navigateTo(path: string, kind: NavKind = 'new') {
    const logStore = useLogStore();
    loading.value = true;
    error.value = null;

    try {
      const result = await invoke<FileEntry[]>('list_local_dir', { path });
      if (path !== currentPath.value) {
        selected.value = [];
      } else {
        // Refresh: keep the selection for entries that still exist.
        const still = new Set(result.map((f) => f.path));
        selected.value = selected.value.filter((p) => still.has(p));
      }
      files.value = result;
      history.moved(currentPath.value, path, kind);
      currentPath.value = path;
      logStore.addLog('Info', t.log.localNav, t.log.ok, t.log.listed(path, result.length));
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', t.log.localNav, t.log.failed, `${path} — ${message}`);
      error.value = message;
    } finally {
      loading.value = false;
    }
  }

  async function navigateUp() {
    if (!currentPath.value) return;
    // Resolved by the backend so Windows paths (C:\Users\me) work too.
    const parent = await invoke<string | null>('get_parent_dir', { path: currentPath.value });
    if (parent) {
      await navigateTo(parent);
    }
  }

  async function goBack() {
    const target = history.previous();
    // One step per click, even if the listing is still loading.
    if (target && !loading.value) await navigateTo(target, 'back');
  }

  async function goForward() {
    const target = history.next();
    if (target && !loading.value) await navigateTo(target, 'forward');
  }

  async function refresh() {
    if (currentPath.value) {
      await navigateTo(currentPath.value);
    }
  }

  return {
    currentPath,
    files,
    selected,
    loading,
    error,
    initialize,
    navigateTo,
    navigateUp,
    goBack,
    goForward,
    canGoBack: history.canGoBack,
    canGoForward: history.canGoForward,
    refresh,
  };
});
