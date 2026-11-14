import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { AppConfig } from '../types';
import { useLogStore } from './logs';
import { errorMessage } from '../utils/errors';
import { setLanguage, t } from '../i18n';

/// Same as `AppConfig::default()` in Rust; used until the saved settings are read.
export const DEFAULT_SETTINGS: AppConfig = {
  language: 'system',
  theme: 'system',
  concurrent_transfers: 2,
  show_hidden_files: true,
  default_conflict: 'Ask',
};

const systemDark = window.matchMedia('(prefers-color-scheme: dark)');

export const useSettingsStore = defineStore('settings', () => {
  const settings = ref<AppConfig>({ ...DEFAULT_SETTINGS });

  function apply() {
    setLanguage(settings.value.language);
    applyTheme();
  }

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
        t.settings.log,
        t.settings.unreadable,
        `${t.settings.usingDefaults} — ${errorMessage(err)}`,
      );
    }
    apply();
  }

  /// Returns false when saving failed; the previous settings then stay in effect.
  async function save(next: AppConfig): Promise<boolean> {
    try {
      settings.value = await invoke<AppConfig>('save_settings', { settings: next });
      apply();
      useLogStore().addLog('Info', t.settings.log, t.settings.ok, t.settings.saved);
      return true;
    } catch (err) {
      useLogStore().addLog('Error', t.settings.log, t.settings.failed, errorMessage(err));
      return false;
    }
  }

  return { settings, load, save };
});
