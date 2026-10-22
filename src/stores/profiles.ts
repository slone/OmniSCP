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

  return {
    profiles,
    selectedId,
    loading,
    selectedProfile,
    loadProfiles,
    saveProfile,
    deleteProfile,
    selectProfile,
  };
});
