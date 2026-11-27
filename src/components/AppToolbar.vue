<script setup lang="ts">
import { ref } from 'vue';
import { useProfilesStore } from '../stores/profiles';
import { useConnectionStore } from '../stores/connection';
import { useLogStore } from '../stores/logs';
import PasswordDialog from './PasswordDialog.vue';
import HostKeyDialog from './HostKeyDialog.vue';
import SyncDialog from './sync/SyncDialog.vue';
import SettingsDialog from './SettingsDialog.vue';
import { useLocalFilesStore } from '../stores/localFiles';
import { isBackendError } from '../utils/errors';
import { useFileActionsStore } from '../stores/fileActions';
import type { Profile } from '../types';
import { t } from '../i18n';

const profileStore = useProfilesStore();
const connectionStore = useConnectionStore();
const logStore = useLogStore();
const actions = useFileActionsStore();
const localFiles = useLocalFilesStore();

const showSync = ref(false);
const showSettings = ref(false);

const showPasswordDialog = ref(false);
const passwordPromptLabel = ref('');
const passwordNotice = ref<string | undefined>();

interface Attempt {
  profile: Profile;
  /// Undefined: the backend uses the password saved in the OS credential store.
  password?: string;
  /// Save `password` in the credential store once it is proven to work.
  remember: boolean;
}

// Pending connection waiting for the user to accept an unknown host key.
const hostKeyPrompt = ref<(Attempt & { fingerprint: string }) | null>(null);

function needsSecret(profile: Profile) {
  const type = profile.auth_method.type;
  return type === 'Password' || type === 'KeyWithPassphrase';
}

function promptPassword(profile: Profile, notice?: string) {
  passwordPromptLabel.value = profile.auth_method.type === 'Password' ? t.auth.password : t.auth.passphrase;
  passwordNotice.value = notice;
  showPasswordDialog.value = true;
}

async function attempt(a: Attempt, trustedFingerprint?: string) {
  try {
    await connectionStore.connect(a.profile, a.password, trustedFingerprint);
  } catch (err) {
    if (isBackendError(err) && err.kind === 'HostKeyUnknown' && err.fingerprint) {
      hostKeyPrompt.value = { ...a, fingerprint: err.fingerprint };
    } else if (isBackendError(err) && err.code === 'credentials.missing') {
      logStore.addLog('Warning', t.connection.keychainLog, t.connection.keychainMissing, t.connection.noSavedPassword(a.profile.name));
      promptPassword(a.profile);
    } else if (isBackendError(err) && err.kind === 'Auth' && a.password === undefined && needsSecret(a.profile)) {
      promptPassword(a.profile, t.connection.savedPasswordRefused);
    }
    // Other errors are already logged by the store.
    return;
  }

  if (a.remember && a.password !== undefined) {
    await profileStore.saveProfile({ ...a.profile, save_password: true }, a.password);
  }
}

function onHostKeyAccept() {
  const prompt = hostKeyPrompt.value;
  hostKeyPrompt.value = null;
  if (!prompt) return;
  attempt(prompt, prompt.fingerprint);
}

function onHostKeyCancel() {
  hostKeyPrompt.value = null;
  logStore.addLog('Info', t.connection.log, t.connection.cancelled, t.connection.hostKeyRefused);
}

async function onConnect() {
  const profile = profileStore.selectedProfile;
  if (!profile) return;

  // A saved password is read by the backend while connecting, not checked
  // first: each keychain read may ask the user's permission.
  if (!needsSecret(profile) || profile.save_password) {
    attempt({ profile, remember: false });
  } else {
    promptPassword(profile);
  }
}

function onPasswordSubmit(password: string, remember: boolean) {
  showPasswordDialog.value = false;
  const profile = profileStore.selectedProfile;
  if (!profile) return;
  attempt({ profile, password, remember });
}

function onPasswordCancel() {
  showPasswordDialog.value = false;
  logStore.addLog('Info', t.connection.log, t.connection.cancelled, t.connection.passwordCancelled);
}

function onDisconnect() {
  connectionStore.disconnect();
}
</script>

<template>
  <div class="toolbar" role="toolbar" :aria-label="t.toolbar.label">
    <div class="toolbar-group">
      <button
        class="toolbar-btn"
        :disabled="!profileStore.selectedProfile || connectionStore.connected || connectionStore.connecting || !!hostKeyPrompt"
        @click="onConnect"
      >
        <span class="icon" aria-hidden="true">&#9889;</span>
        {{ connectionStore.connecting ? t.toolbar.connecting : t.toolbar.connect }}
      </button>
      <button
        class="toolbar-btn"
        :disabled="!connectionStore.connected"
        @click="onDisconnect"
      >
        <span class="icon" aria-hidden="true">&#10005;</span>
        {{ t.toolbar.disconnect }}
      </button>
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group">
      <button
        class="toolbar-btn"
        :title="t.toolbar.mkdir"
        :aria-label="t.toolbar.mkdir"
        :disabled="!actions.canMkdir"
        @click="actions.requestMkdir"
      >
        <span class="icon" aria-hidden="true">&#128193;</span>
      </button>
      <button
        class="toolbar-btn"
        :title="t.toolbar.rename"
        :aria-label="t.toolbar.rename"
        :disabled="!actions.canRename"
        @click="actions.requestRename"
      >
        <span class="icon" aria-hidden="true">&#9998;&#65039;</span>
      </button>
      <button
        class="toolbar-btn"
        :title="t.toolbar.delete"
        :aria-label="t.toolbar.delete"
        :disabled="!actions.canDelete"
        @click="actions.requestDelete"
      >
        <span class="icon" aria-hidden="true">&#128465;</span>
      </button>
      <button
        class="toolbar-btn"
        :title="t.toolbar.properties"
        :aria-label="t.toolbar.properties"
        :disabled="!actions.canShowProperties"
        @click="actions.requestProperties"
      >
        <span class="icon" aria-hidden="true">&#9432;</span>
      </button>
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group">
      <button
        class="toolbar-btn"
        :title="t.toolbar.uploadHint"
        :disabled="!actions.canUpload"
        @click="actions.transfer('local')"
      >
        <span class="icon" aria-hidden="true">&uarr;</span>
        {{ t.toolbar.upload }}
      </button>
      <button
        class="toolbar-btn"
        :title="t.toolbar.downloadHint"
        :disabled="!actions.canDownload"
        @click="actions.transfer('remote')"
      >
        <span class="icon" aria-hidden="true">&darr;</span>
        {{ t.toolbar.download }}
      </button>
      <button
        class="toolbar-btn"
        :title="t.toolbar.moveHint"
        :disabled="!actions.canMove"
        @click="actions.requestMove()"
      >
        <span class="icon" aria-hidden="true">&#8669;</span>
        {{ t.toolbar.move }}
      </button>
      <button
        class="toolbar-btn"
        :title="t.toolbar.editHint"
        :disabled="!actions.canEdit"
        @click="actions.requestEdit"
      >
        <span class="icon" aria-hidden="true">&#128221;</span>
        {{ t.toolbar.edit }}
      </button>
      <button
        class="toolbar-btn"
        :title="t.toolbar.syncHint"
        :disabled="!connectionStore.connected || !connectionStore.currentPath || !localFiles.currentPath"
        @click="showSync = true"
      >
        <span class="icon" aria-hidden="true">&#8644;</span>
        {{ t.toolbar.sync }}
      </button>
    </div>

    <div class="toolbar-spacer" />

    <div class="toolbar-group">
      <button
        class="toolbar-btn"
        :title="t.toolbar.settings"
        :aria-label="t.toolbar.settings"
        @click="showSettings = true"
      >
        <span class="icon" aria-hidden="true">&#9881;</span>
      </button>
    </div>
  </div>

  <SyncDialog v-if="showSync" @close="showSync = false" />
  <SettingsDialog v-if="showSettings" @close="showSettings = false" />

  <PasswordDialog
    v-if="showPasswordDialog"
    :profile-name="profileStore.selectedProfile?.name ?? ''"
    :prompt-label="passwordPromptLabel"
    :notice="passwordNotice"
    :remember-default="profileStore.selectedProfile?.save_password ?? false"
    @submit="onPasswordSubmit"
    @cancel="onPasswordCancel"
  />

  <HostKeyDialog
    v-if="hostKeyPrompt"
    :profile-name="hostKeyPrompt.profile.name"
    :host="hostKeyPrompt.profile.host"
    :port="hostKeyPrompt.profile.port"
    :fingerprint="hostKeyPrompt.fingerprint"
    :certificate="hostKeyPrompt.profile.protocol === 'Ftps'"
    @accept="onHostKeyAccept"
    @cancel="onHostKeyCancel"
  />
</template>

<style scoped>
.toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
  min-height: 36px;
}

.toolbar-group {
  display: flex;
  align-items: center;
  gap: 2px;
}

.toolbar-separator {
  width: 1px;
  height: 20px;
  background: var(--color-border);
  margin: 0 4px;
}

.toolbar-spacer {
  flex: 1;
}

.toolbar-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 4px 8px;
  border: 1px solid transparent;
  border-radius: 3px;
  background: none;
  color: var(--color-text);
  font-size: 12px;
  cursor: pointer;
  white-space: nowrap;
}

.toolbar-btn:hover:not(:disabled) {
  background: var(--color-hover);
  border-color: var(--color-border);
}

.toolbar-btn:active:not(:disabled) {
  background: var(--color-active);
}

.toolbar-btn:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: -1px;
}

.toolbar-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.icon {
  font-size: 14px;
}
</style>
