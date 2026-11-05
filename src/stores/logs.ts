import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { LogEntry, LogLevel } from '../types';
import { errorMessage } from '../utils/errors';

/// The journal file holds more; this only bounds what the panel renders.
const MAX_DISPLAYED = 2000;
const LOADED_AT_STARTUP = 1000;

export const useLogStore = defineStore('logs', () => {
  const entries = ref<LogEntry[]>([]);
  /// Writing to the journal file failed, so entries only live until the app quits.
  const saveError = ref<string | null>(null);

  async function load() {
    try {
      entries.value = await invoke<LogEntry[]>('load_logs', { limit: LOADED_AT_STARTUP });
    } catch (err) {
      saveError.value = errorMessage(err);
    }
  }

  function addLog(level: LogLevel, action: string, status: string, detail?: string) {
    const entry: LogEntry = {
      timestamp: Math.floor(Date.now() / 1000),
      level,
      action,
      status,
      detail,
    };
    entries.value.push(entry);
    if (entries.value.length > MAX_DISPLAYED) {
      entries.value.splice(0, entries.value.length - MAX_DISPLAYED);
    }
    invoke('append_log', { entry }).then(
      () => {
        saveError.value = null;
      },
      (err) => {
        saveError.value = errorMessage(err);
      },
    );
  }

  async function clear() {
    await invoke('clear_logs');
    entries.value = [];
  }

  /// Returns the path of the exported file.
  async function exportToDownloads() {
    return invoke<string>('export_logs');
  }

  return { entries, saveError, load, addLog, clear, exportToDownloads };
});
