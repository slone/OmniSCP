import { ref } from 'vue';
import { defineStore } from 'pinia';
import type { LogEntry, LogLevel } from '../types';

export const useLogStore = defineStore('logs', () => {
  const entries = ref<LogEntry[]>([]);

  function addLog(level: LogLevel, action: string, status: string, detail?: string) {
    entries.value.push({
      timestamp: Math.floor(Date.now() / 1000),
      level,
      action,
      status,
      detail,
    });
  }

  function clear() {
    entries.value = [];
  }

  return { entries, addLog, clear };
});
