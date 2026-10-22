import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { FileEntry } from '../types';
import { useLogStore } from './logs';
import { errorMessage } from '../utils/errors';

export const useLocalFilesStore = defineStore('localFiles', () => {
  const currentPath = ref('');
  const files = ref<FileEntry[]>([]);
  const selected = ref<string[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function initialize() {
    try {
      const home = await invoke<string>('get_home_dir');
      await navigateTo(home);
    } catch (err) {
      const logStore = useLogStore();
      const message = errorMessage(err);
      logStore.addLog('Error', 'Initialisation', 'Échec', message);
      error.value = message;
    }
  }

  async function navigateTo(path: string) {
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
      currentPath.value = path;
      logStore.addLog('Info', 'Navigation', 'OK', `${path} — ${result.length} élément(s)`);
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', 'Navigation', 'Échec', `${path} — ${message}`);
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

  function refresh() {
    if (currentPath.value) {
      navigateTo(currentPath.value);
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
    refresh,
  };
});
