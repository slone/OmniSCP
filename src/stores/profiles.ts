import { ref, computed } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { Profile } from '../types';
import { useLogStore } from './logs';
import { errorMessage } from '../utils/errors';
import { t } from '../i18n';

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
      useLogStore().addLog('Error', t.log.profiles, t.log.failed, `${what} — ${errorMessage(err)}`);
      return false;
    }
  }

  async function loadProfiles() {
    const logStore = useLogStore();
    loading.value = true;

    try {
      const result = await invoke<Profile[]>('load_profiles');
      profiles.value = result;
      logStore.addLog('Info', t.log.profiles, t.log.ok, t.log.profilesLoaded(result.length));
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', t.log.profiles, t.log.failed, `${t.log.loading} — ${message}`);
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
          t.log.keychain,
          t.log.ok,
          secret === null ? t.log.passwordForgotten(profile.name) : t.log.passwordSaved(profile.name),
        );
      } catch (err) {
        logStore.addLog('Error', t.log.profiles, t.log.failed, `${t.log.keychain} — ${errorMessage(err)}`);
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
        t.log.profiles,
        t.log.ok,
        index >= 0 ? t.log.profileChanged(profile.name) : t.log.profileCreated(profile.name),
      );
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', t.log.profiles, t.log.failed, `${t.log.saving} — ${message}`);
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
        logStore.addLog('Error', t.log.profiles, t.log.failed, `${t.log.keychain} — ${errorMessage(err)}`),
      );
    }

    try {
      await invoke('save_profiles', { profiles: profiles.value });
      logStore.addLog('Info', t.log.profiles, t.log.ok, t.log.profileDeleted(name));
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', t.log.profiles, t.log.failed, `${t.log.deleting} — ${message}`);
    }
  }

  function selectProfile(id: string | null) {
    selectedId.value = id;
  }

  /// Adds already-checked profiles (from an import), all saved at once.
  async function addProfiles(added: Profile[]) {
    if (added.length === 0) return;
    profiles.value.push(...added);
    if (await persist(t.log.importing)) {
      useLogStore().addLog('Info', t.log.profiles, t.log.ok, t.log.profilesImported(added.length));
    }
  }

  /// Moves every profile of a group to another name; an empty name ungroups them.
  async function renameGroup(from: string, to: string) {
    const target = to.trim() || undefined;
    if (target === from) return;
    profiles.value = profiles.value.map((p) => (p.group === from ? { ...p, group: target } : p));
    if (await persist(t.log.group)) {
      useLogStore().addLog(
        'Info',
        t.log.profiles,
        t.log.ok,
        target ? t.log.groupRenamed(from, target) : t.log.groupRemoved(from),
      );
    }
  }

  /// Writes the profiles to a file for another machine; passwords are never included.
  async function exportProfiles(list: Profile[], path: string): Promise<boolean> {
    const logStore = useLogStore();
    try {
      const count = await invoke<number>('export_profiles', { path, profiles: list });
      logStore.addLog('Info', t.log.profiles, t.log.ok, t.log.profilesExported(count, path));
      return true;
    } catch (err) {
      logStore.addLog('Error', t.log.profiles, t.log.failed, `${t.log.exporting} — ${errorMessage(err)}`);
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
