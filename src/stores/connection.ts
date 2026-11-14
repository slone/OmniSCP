import { ref } from 'vue';
import { defineStore } from 'pinia';
import { invoke } from '@tauri-apps/api/core';
import type { FileEntry, Profile } from '../types';
import { useLogStore } from './logs';
import { useNavHistory, type NavKind } from '../utils/history';
import { t } from '../i18n';
import { errorMessage, isBackendError } from '../utils/errors';
import { useTransfersStore } from './transfers';
import { useEditsStore } from './edits';

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
  const history = useNavHistory();

  async function connect(profile: Profile, password?: string, trustedFingerprint?: string) {
    const logStore = useLogStore();
    connecting.value = true;
    error.value = null;

    try {
      const initialPath = await invoke<string>('connect', {
        profile,
        password,
        trustedFingerprint,
      });
      connected.value = true;
      profileId.value = profile.id;
      profileName.value = profile.name;
      logStore.addLog('Info', t.connection.log, t.log.ok, t.log.connectedTo(profile.name));
      await navigateTo(initialPath);
    } catch (err) {
      if (isBackendError(err) && err.kind === 'HostKeyUnknown') {
        // Not a failure yet: the caller asks the user to confirm the fingerprint.
        logStore.addLog('Warning', t.connection.log, t.log.unknownKey, `${profile.name} — ${err.fingerprint}`);
        throw err;
      }
      const message =
        isBackendError(err) && err.kind === 'HostKeyMismatch'
          ? t.log.hostKeyChanged(err.fingerprint ?? '')
          : errorMessage(err);
      logStore.addLog('Error', t.connection.log, t.log.failed, `${profile.name} — ${message}`);
      error.value = message;
      throw err;
    } finally {
      connecting.value = false;
    }
  }

  async function disconnect() {
    const logStore = useLogStore();
    try {
      await invoke('disconnect');
      logStore.addLog('Info', t.log.disconnect, t.log.ok, t.log.disconnectedFrom(profileName.value ?? ''));
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', t.log.disconnect, t.log.failed, message);
    } finally {
      useTransfersStore().cancelAllLocally();
      useEditsStore().reset();
      connected.value = false;
      connecting.value = false;
      profileId.value = null;
      profileName.value = null;
      currentPath.value = '';
      history.reset();
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
    error.value = t.log.connectionLost;
  }

  async function navigateTo(path: string, kind: NavKind = 'new') {
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
      history.moved(currentPath.value, path, kind);
      currentPath.value = path;
      logStore.addLog('Info', t.log.remoteNav, t.log.ok, t.log.listed(path, result.length));
    } catch (err) {
      const message = errorMessage(err);
      logStore.addLog('Error', t.log.remoteNav, t.log.failed, `${path} — ${message}`);
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
    goBack,
    goForward,
    canGoBack: history.canGoBack,
    canGoForward: history.canGoForward,
    refresh,
  };
});
