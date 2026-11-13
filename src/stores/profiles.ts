import { ref, computed } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { Profile } from '../types';
import { useLogStore } from './logs';
import { errorMessage } from '../utils/errors';

export const useProfilesStore = defineStore('profiles', () => {
  const profiles = ref<Profile[]>([]);
  const selectedId = ref<string | null>(null);
  const loading = ref(false);

  const selectedProfile = computed(() =>
    profiles.value.find((p) => p.id === selectedId.value) ?? null,
  );

  const collator = new Intl.Collator('fr', { numeric: true, sensitivity: 'base' });

  /// Group names in use, sorted.
  const groups = computed(() =>
    [...new Set(profiles.value.map((p) => p.group).filter((g): g is string => !!g))].sort(collator.compare),
  );

  /// Writes the whole list. Returns false when it could not be saved.
  async function persist(what: string): Promise<boolean> {
    try {
      await invoke('save_profiles', { profiles: profiles.value });
      return true;
    } catch (err) {
      useLogStore().addLog('Error', 'Profils', 'Échec', `${what} — ${errorMessage(err)}`);
      return false;
    }
  }

  async function loadProfiles() {
    const logStore = useLogStore();
    loading.value = true;

    try {
      const result = await invoke<Profile[]>('load_profiles');
      profiles.value = result;
      logStore.addLog('Info', 'Profils', 'OK', `${result.length} profil(s) chargé(s)`);
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', 'Profils', 'Échec', `Chargement — ${message}`);
    } finally {
      loading.value = false;
    }
  }

  /// secret: string = store it, null = forget it, undefined = leave it as is.
  async function saveProfile(profile: Profile, secret?: string | null) {
    const logStore = useLogStore();

    if (secret !== undefined) {
      try {
        await invoke('set_saved_password', { profileId: profile.id, password: secret });
        logStore.addLog(
          'Info',
          'Trousseau',
          'OK',
          secret === null
            ? `Mot de passe de « ${profile.name} » supprimé`
            : `Mot de passe de « ${profile.name} » enregistré`,
        );
      } catch (err) {
        logStore.addLog('Error', 'Profils', 'Échec', `Trousseau — ${errorMessage(err)}`);
        profile = { ...profile, save_password: false };
      }
    }

    const index = profiles.value.findIndex((p) => p.id === profile.id);

    if (index >= 0) {
      profiles.value[index] = profile;
    } else {
      profiles.value.push(profile);
    }

    try {
      await invoke('save_profiles', { profiles: profiles.value });
      logStore.addLog(
        'Info',
        'Profils',
        'OK',
        `Profil « ${profile.name} » ${index >= 0 ? 'modifié' : 'créé'}`,
      );
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', 'Profils', 'Échec', `Sauvegarde — ${message}`);
    }
  }

  async function deleteProfile(id: string) {
    const logStore = useLogStore();
    const profile = profiles.value.find((p) => p.id === id);
    const name = profile?.name ?? id;

    profiles.value = profiles.value.filter((p) => p.id !== id);

    if (selectedId.value === id) {
      selectedId.value = null;
    }

    if (profile?.save_password) {
      await invoke('set_saved_password', { profileId: id, password: null }).catch((err) =>
        logStore.addLog('Error', 'Profils', 'Échec', `Trousseau — ${errorMessage(err)}`),
      );
    }

    try {
      await invoke('save_profiles', { profiles: profiles.value });
      logStore.addLog('Info', 'Profils', 'OK', `Profil « ${name} » supprimé`);
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', 'Profils', 'Échec', `Suppression — ${message}`);
    }
  }

  function selectProfile(id: string | null) {
    selectedId.value = id;
  }

  /// Adds already-checked profiles (from an import), all saved at once.
  async function addProfiles(added: Profile[]) {
    if (added.length === 0) return;
    profiles.value.push(...added);
    if (await persist('Import')) {
      useLogStore().addLog('Info', 'Profils', 'OK', `${added.length} profil(s) importé(s)`);
    }
  }

  /// Moves every profile of a group to another name; an empty name ungroups them.
  async function renameGroup(from: string, to: string) {
    const target = to.trim() || undefined;
    if (target === from) return;
    profiles.value = profiles.value.map((p) => (p.group === from ? { ...p, group: target } : p));
    if (await persist('Groupe')) {
      useLogStore().addLog(
        'Info',
        'Profils',
        'OK',
        target ? `Groupe « ${from} » renommé en « ${target} »` : `Groupe « ${from} » supprimé, ses profils sont conservés`,
      );
    }
  }

  /// Writes the profiles to a file for another machine; passwords are never included.
  async function exportProfiles(list: Profile[], path: string): Promise<boolean> {
    const logStore = useLogStore();
    try {
      const count = await invoke<number>('export_profiles', { path, profiles: list });
      logStore.addLog('Info', 'Profils', 'OK', `${count} profil(s) exporté(s) vers ${path}`);
      return true;
    } catch (err) {
      logStore.addLog('Error', 'Profils', 'Échec', `Export — ${errorMessage(err)}`);
      return false;
    }
  }

  return {
    profiles,
    selectedId,
    loading,
    selectedProfile,
    groups,
    loadProfiles,
    addProfiles,
    renameGroup,
    exportProfiles,
    saveProfile,
    deleteProfile,
    selectProfile,
  };
});
