import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { FileEntry, Profile } from '../types';
import { useLogStore } from './logs';
import { errorMessage, isBackendError } from '../utils/errors';
import { useTransfersStore } from './transfers';

export const useConnectionStore = defineStore('connection', () => {
  const connected = ref(false);
  const connecting = ref(false);
  const profileId = ref<string | null>(null);
  const profileName = ref<string | null>(null);
  const currentPath = ref('');
  const files = ref<FileEntry[]>([]);
  const selected = ref<string[]>([]);
  const loading = ref(false);
  const error = ref<string | null>(null);

  async function connect(profile: Profile, password?: string, trustedFingerprint?: string) {
    const logStore = useLogStore();
    connecting.value = true;
    error.value = null;

    try {
      const initialPath = await invoke<string>('connect_sftp', {
        profile,
        password,
        trustedFingerprint,
      });
      connected.value = true;
      profileId.value = profile.id;
      profileName.value = profile.name;
      logStore.addLog('Info', 'Connexion', 'OK', `Connecté à ${profile.name}`);
      await navigateTo(initialPath);
    } catch (err) {
      if (isBackendError(err) && err.kind === 'HostKeyUnknown') {
        // Not a failure yet: the caller asks the user to confirm the fingerprint.
        logStore.addLog('Warning', 'Connexion', 'Clé inconnue', `${profile.name} — ${err.fingerprint}`);
        throw err;
      }
      const message =
        isBackendError(err) && err.kind === 'HostKeyMismatch'
          ? `ATTENTION : la clé d'hôte a changé (${err.fingerprint}). Connexion refusée — possible attaque de l'homme du milieu.`
          : errorMessage(err);
      logStore.addLog('Error', 'Connexion', 'Échec', `${profile.name} — ${message}`);
      error.value = message;
      throw err;
    } finally {
      connecting.value = false;
    }
  }

  async function disconnect() {
    const logStore = useLogStore();
    try {
      await invoke('disconnect_sftp');
      logStore.addLog('Info', 'Déconnexion', 'OK', `Déconnecté de ${profileName.value}`);
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', 'Déconnexion', 'Échec', message);
    } finally {
      useTransfersStore().cancelAllLocally();
      connected.value = false;
      connecting.value = false;
      profileId.value = null;
      profileName.value = null;
      currentPath.value = '';
      files.value = [];
      selected.value = [];
      loading.value = false;
      error.value = null;
    }
  }

  /// Leaves the connected state if `err` says the session died; other errors are ignored.
  async function handleConnectionLost(err: unknown) {
    if (!isBackendError(err) || err.kind !== 'ConnectionLost' || !connected.value) return;
    await disconnect();
    error.value = 'La connexion au serveur a été perdue. Reconnectez-vous pour continuer.';
  }

  async function navigateTo(path: string) {
    const logStore = useLogStore();
    loading.value = true;
    error.value = null;

    try {
      const result = await invoke<FileEntry[]>('list_remote_dir', { path });
      if (path !== currentPath.value) {
        selected.value = [];
      } else {
        // Refresh: keep the selection for entries that still exist.
        const still = new Set(result.map((f) => f.path));
        selected.value = selected.value.filter((p) => still.has(p));
      }
      files.value = result;
      currentPath.value = path;
      logStore.addLog('Info', 'Navigation distante', 'OK', `${path} — ${result.length} élément(s)`);
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', 'Navigation distante', 'Échec', `${path} — ${message}`);
      error.value = message;
      await handleConnectionLost(err);
    } finally {
      loading.value = false;
    }
  }

  function navigateUp() {
    if (!currentPath.value || currentPath.value === '/') return;
    const parent = currentPath.value.replace(/\/[^/]+\/?$/, '') || '/';
    navigateTo(parent);
  }

  function refresh() {
    if (currentPath.value) {
      navigateTo(currentPath.value);
    }
  }

  return {
    connected,
    connecting,
    profileId,
    profileName,
    currentPath,
    files,
    selected,
    loading,
    error,
    connect,
    disconnect,
    handleConnectionLost,
    navigateTo,
    navigateUp,
    refresh,
  };
});
