<script setup lang="ts">
import { ref } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useProfilesStore } from '../stores/profiles';
import { useConnectionStore } from '../stores/connection';
import { useLogStore } from '../stores/logs';
import PasswordDialog from './PasswordDialog.vue';
import HostKeyDialog from './HostKeyDialog.vue';
import { isBackendError } from '../utils/errors';
import { useFileActionsStore } from '../stores/fileActions';
import type { Profile } from '../types';

const profileStore = useProfilesStore();
const connectionStore = useConnectionStore();
const logStore = useLogStore();
const actions = useFileActionsStore();

const showPasswordDialog = ref(false);
const passwordPromptLabel = ref('Mot de passe');
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
  passwordPromptLabel.value = profile.auth_method.type === 'Password' ? 'Mot de passe' : 'Passphrase';
  passwordNotice.value = notice;
  showPasswordDialog.value = true;
}

async function attempt(a: Attempt, trustedFingerprint?: string) {
  try {
    await connectionStore.connect(a.profile, a.password, trustedFingerprint);
  } catch (err) {
    if (isBackendError(err) && err.kind === 'HostKeyUnknown' && err.fingerprint) {
      hostKeyPrompt.value = { ...a, fingerprint: err.fingerprint };
    } else if (isBackendError(err) && err.kind === 'Auth' && a.password === undefined && needsSecret(a.profile)) {
      promptPassword(a.profile, 'Le mot de passe enregistré a été refusé. Saisissez-le à nouveau.');
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
  logStore.addLog('Info', 'Connexion', 'Annulée', 'Empreinte du serveur refusée');
}

async function onConnect() {
  const profile = profileStore.selectedProfile;
  if (!profile) return;

  if (!needsSecret(profile)) {
    attempt({ profile, remember: false });
    return;
  }

  const saved =
    profile.save_password &&
    (await invoke<boolean>('has_saved_password', { profileId: profile.id }).catch(() => false));
  if (saved) {
    attempt({ profile, remember: false });
  } else {
    if (profile.save_password) {
      logStore.addLog('Warning', 'Trousseau', 'Absent', `Aucun mot de passe enregistré trouvé pour « ${profile.name} »`);
    }
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
  logStore.addLog('Info', 'Connexion', 'Annulée', 'Saisie du mot de passe annulée');
}

function onDisconnect() {
  connectionStore.disconnect();
}
</script>

<template>
  <div class="toolbar">
    <div class="toolbar-group">
      <button
        class="toolbar-btn"
        title="Connexion"
        :disabled="!profileStore.selectedProfile || connectionStore.connected || connectionStore.connecting || !!hostKeyPrompt"
        @click="onConnect"
      >
        <span class="icon">&#9889;</span>
        <template v-if="connectionStore.connecting">Connexion...</template>
        <template v-else>Connexion</template>
      </button>
      <button
        class="toolbar-btn"
        title="Déconnexion"
        :disabled="!connectionStore.connected"
        @click="onDisconnect"
      >
        <span class="icon">&#10005;</span>
        Déconnexion
      </button>
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group">
      <button class="toolbar-btn" title="Nouveau dossier (F7)" :disabled="!actions.canMkdir" @click="actions.requestMkdir">
        <span class="icon">&#128193;</span>
      </button>
      <button class="toolbar-btn" title="Renommer (F2)" :disabled="!actions.canRename" @click="actions.requestRename">
        <span class="icon">&#9998;&#65039;</span>
      </button>
      <button class="toolbar-btn" title="Supprimer (Suppr)" :disabled="!actions.canDelete" @click="actions.requestDelete">
        <span class="icon">&#128465;</span>
      </button>
    </div>

    <div class="toolbar-separator" />

    <div class="toolbar-group">
      <button
        class="toolbar-btn"
        title="Envoyer la sélection locale vers le serveur (F5)"
        :disabled="!actions.canUpload"
        @click="actions.transfer('local')"
      >
        <span class="icon">&uarr;</span>
        Upload
      </button>
      <button
        class="toolbar-btn"
        title="Télécharger la sélection distante (F5)"
        :disabled="!actions.canDownload"
        @click="actions.transfer('remote')"
      >
        <span class="icon">&darr;</span>
        Download
      </button>
    </div>

    <div class="toolbar-spacer" />

    <div class="toolbar-group">
      <button class="toolbar-btn" title="Paramètres">
        <span class="icon">&#9881;</span>
      </button>
    </div>
  </div>

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

.toolbar-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.icon {
  font-size: 14px;
}
</style>
