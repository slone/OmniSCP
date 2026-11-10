import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig } from '../types';
import { useLogStore } from './logs';
import { errorMessage } from '../utils/errors';

/// Same as `AppConfig::default()` in Rust; used until the saved settings are read.
export const DEFAULT_SETTINGS: AppConfig = {
  language: 'fr',
  theme: 'system',
  concurrent_transfers: 2,
  show_hidden_files: true,
  default_conflict: 'Ask',
};

const systemDark = window.matchMedia('(prefers-color-scheme: dark)');

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<AppConfig>({ ...DEFAULT_SETTINGS });

  function applyTheme() {
    const theme = settings.value.theme;
    const dark = theme === 'dark' || (theme === 'system' && systemDark.matches);
    document.documentElement.dataset.theme = dark ? 'dark' : 'light';
  }

  systemDark.addEventListener('change', applyTheme);

  async function load() {
    try {
      settings.value = await invoke<AppConfig>('load_settings');
    } catch (err) {
      useLogStore().addLog(
        'Warning',
        'Paramètres',
        'Illisibles',
        `Paramètres par défaut utilisés — ${errorMessage(err)}`,
      );
    }
    applyTheme();
  }

  /// Returns false when saving failed; the previous settings then stay in effect.
  async function save(next: AppConfig): Promise<boolean> {
    try {
      settings.value = await invoke<AppConfig>('save_settings', { settings: next });
      applyTheme();
      useLogStore().addLog('Info', 'Paramètres', 'OK', 'Paramètres enregistrés');
      return true;
    } catch (err) {
      useLogStore().addLog('Error', 'Paramètres', 'Échec', errorMessage(err));
      return false;
    }
  }

  return { settings, load, save };
});
