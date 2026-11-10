<script setup lang="ts">
import { ref, watch, computed, onMounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import type { Profile, ProtocolType, AuthMethod } from '../../types';

const props = defineProps<{
  profile?: Profile;
}>();

const emit = defineEmits<{
  /// secret: string = store it, null = forget it, undefined = keep the saved one.
  save: [profile: Profile, secret: string | null | undefined];
  cancel: [];
}>();

const allProtocols: ProtocolType[] = ['Sftp', 'Scp', 'Ftp', 'Ftps', 'WebDav', 'WebDavs'];

const availableProtocols: ProtocolType[] = ['Sftp', 'Scp'];

const defaultPorts: Record<ProtocolType, number> = {
  Sftp: 22,
  Scp: 22,
  Ftp: 21,
  Ftps: 990,
  WebDav: 80,
  WebDavs: 443,
};

type AuthType = 'Password' | 'KeyFile' | 'KeyWithPassphrase' | 'Agent';
const allAuthTypes: { value: AuthType; label: string }[] = [
  { value: 'Password', label: 'Mot de passe' },
  { value: 'KeyFile', label: 'Clé SSH' },
  { value: 'KeyWithPassphrase', label: 'Clé + passphrase' },
  { value: 'Agent', label: 'Agent SSH' },
];

const isEditing = computed(() => !!props.profile);

const name = ref(props.profile?.name ?? '');
const host = ref(props.profile?.host ?? '');
const port = ref(props.profile?.port ?? 22);
const protocol = ref<ProtocolType>(props.profile?.protocol ?? 'Sftp');
const username = ref(props.profile?.username ?? '');
const authType = ref<AuthType>(props.profile?.auth_method.type ?? 'Password');
const keyPath = ref(
  props.profile?.auth_method.type === 'KeyFile' || props.profile?.auth_method.type === 'KeyWithPassphrase'
    ? props.profile.auth_method.path
    : '',
);
const remoteDirectory = ref(props.profile?.remote_directory ?? '');
const description = ref(props.profile?.description ?? '');
const tagsText = ref(props.profile?.tags.join(', ') ?? '');

const portManuallyChanged = ref(false);

watch(protocol, (newProto) => {
  if (!portManuallyChanged.value) {
    port.value = defaultPorts[newProto];
  }
});

function onPortInput() {
  portManuallyChanged.value = true;
}

const showKeyPath = computed(
  () => authType.value === 'KeyFile' || authType.value === 'KeyWithPassphrase',
);

const needsSecret = computed(
  () => authType.value === 'Password' || authType.value === 'KeyWithPassphrase',
);
const secretLabel = computed(() => (authType.value === 'Password' ? 'Mot de passe' : 'Passphrase'));
const savePassword = ref(props.profile?.save_password ?? false);
const secret = ref('');
const hasSaved = ref(false);
// A saved password must not silently become a passphrase (or vice versa).
const canKeepSaved = computed(
  () => hasSaved.value && authType.value === props.profile?.auth_method.type,
);

onMounted(async () => {
  if (props.profile?.save_password) {
    hasSaved.value = await invoke<boolean>('has_saved_password', { profileId: props.profile.id }).catch(
      () => false,
    );
  }
});

const validationError = ref('');

async function onSave() {
  validationError.value = '';

  if (!name.value.trim()) {
    validationError.value = 'Le nom du profil est requis.';
    return;
  }
  if (!host.value.trim()) {
    validationError.value = "L'hôte est requis.";
    return;
  }
  if (!username.value.trim()) {
    validationError.value = "Le nom d'utilisateur est requis.";
    return;
  }
  if (showKeyPath.value && !keyPath.value.trim()) {
    validationError.value = 'Le chemin de la clé SSH est requis.';
    return;
  }
  const storeSecret = needsSecret.value && savePassword.value;
  if (storeSecret && !secret.value && !canKeepSaved.value) {
    validationError.value = `Saisissez le ${secretLabel.value.toLowerCase()} à enregistrer.`;
    return;
  }

  let authMethod: AuthMethod;
  switch (authType.value) {
    case 'Password':
      authMethod = { type: 'Password' };
      break;
    case 'KeyFile':
      authMethod = { type: 'KeyFile', path: keyPath.value.trim() };
      break;
    case 'KeyWithPassphrase':
      authMethod = { type: 'KeyWithPassphrase', path: keyPath.value.trim() };
      break;
    case 'Agent':
      authMethod = { type: 'Agent' };
      break;
  }

  const tags = tagsText.value
    .split(',')
    .map((t) => t.trim())
    .filter((t) => t.length > 0);

  let id = props.profile?.id;
  if (!id) {
    id = await invoke<string>('generate_id');
  }

  const profile: Profile = {
    id,
    name: name.value.trim(),
    host: host.value.trim(),
    port: port.value,
    protocol: protocol.value,
    username: username.value.trim(),
    auth_method: authMethod,
    remote_directory: remoteDirectory.value.trim() || undefined,
    description: description.value.trim() || undefined,
    tags,
    save_password: storeSecret,
  };

  const secretChange = !storeSecret ? null : secret.value ? secret.value : undefined;
  emit('save', profile, secretChange);
}

function onOverlayClick(e: MouseEvent) {
  if ((e.target as HTMLElement).classList.contains('dialog-overlay')) {
    emit('cancel');
  }
}
</script>

<template>
  <div class="dialog-overlay" @click="onOverlayClick">
    <div class="dialog-panel">
      <div class="dialog-header">
        <span class="dialog-title">
          {{ isEditing ? 'Modifier le profil' : 'Nouveau profil' }}
        </span>
      </div>

      <div class="dialog-body">
        <div class="form-group">
          <label class="form-label">Nom du profil *</label>
          <input v-model="name" type="text" class="form-input" placeholder="Mon serveur" />
        </div>

        <div class="form-row">
          <div class="form-group form-group-grow">
            <label class="form-label">Hôte *</label>
            <input v-model="host" type="text" class="form-input" placeholder="example.com" />
          </div>
          <div class="form-group form-group-small">
            <label class="form-label">Port</label>
            <input v-model.number="port" type="number" class="form-input" min="1" max="65535" @input="onPortInput" />
          </div>
        </div>

        <div class="form-group">
          <label class="form-label">Protocole</label>
          <select v-model="protocol" class="form-input">
            <option v-for="p in allProtocols" :key="p" :value="p" :disabled="!availableProtocols.includes(p)">
              {{ p }}{{ availableProtocols.includes(p) ? '' : ' (bientôt)' }}
            </option>
          </select>
          <span v-if="protocol === 'Scp'" class="form-hint">
            SCP utilise des commandes shell sur le serveur pour lister, renommer et supprimer. Préférez SFTP s'il est disponible.
          </span>
        </div>

        <div class="form-group">
          <label class="form-label">Nom d'utilisateur *</label>
          <input v-model="username" type="text" class="form-input" placeholder="user" />
        </div>

        <div class="form-group">
          <label class="form-label">Authentification</label>
          <select v-model="authType" class="form-input">
            <option v-for="a in allAuthTypes" :key="a.value" :value="a.value">{{ a.label }}</option>
          </select>
        </div>

        <div v-if="showKeyPath" class="form-group">
          <label class="form-label">Chemin de la clé</label>
          <input v-model="keyPath" type="text" class="form-input" placeholder="~/.ssh/id_rsa" />
        </div>

        <div v-if="needsSecret" class="form-group">
          <label class="checkbox-label">
            <input v-model="savePassword" type="checkbox" />
            Enregistrer le {{ secretLabel.toLowerCase() }}
          </label>
        </div>

        <div v-if="needsSecret && savePassword" class="form-group">
          <label class="form-label">{{ secretLabel }}</label>
          <input
            v-model="secret"
            type="password"
            class="form-input"
            autocomplete="new-password"
            :placeholder="canKeepSaved ? 'Enregistré — laisser vide pour le conserver' : ''"
          />
          <span class="form-hint">Conservé dans le trousseau du système, jamais dans le fichier des profils.</span>
        </div>

        <div class="form-group">
          <label class="form-label">Répertoire distant initial</label>
          <input v-model="remoteDirectory" type="text" class="form-input" placeholder="/var/www" />
        </div>

        <div class="form-group">
          <label class="form-label">Description</label>
          <input v-model="description" type="text" class="form-input" placeholder="Description optionnelle" />
        </div>

        <div class="form-group">
          <label class="form-label">Tags (séparés par des virgules)</label>
          <input v-model="tagsText" type="text" class="form-input" placeholder="prod, web, backup" />
        </div>

        <div v-if="validationError" class="form-error">{{ validationError }}</div>
      </div>

      <div class="dialog-footer">
        <button class="btn btn-cancel" @click="emit('cancel')">Annuler</button>
        <button class="btn btn-save" @click="onSave">Enregistrer</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
}

.dialog-panel {
  width: 420px;
  max-height: 80vh;
  display: flex;
  flex-direction: column;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
}

.dialog-header {
  padding: 12px 16px;
  border-bottom: 1px solid var(--color-border);
}

.dialog-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--color-text);
}

.dialog-body {
  padding: 16px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.form-row {
  display: flex;
  gap: 12px;
}

.form-group-grow {
  flex: 1;
}

.form-group-small {
  width: 80px;
  flex-shrink: 0;
}

.form-label {
  font-size: 11px;
  font-weight: 500;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}

.form-input {
  padding: 6px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}

.form-input:focus {
  border-color: var(--color-accent);
}

.form-input option {
  background: var(--color-bg);
  color: var(--color-text);
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--color-text);
  cursor: pointer;
}

.form-hint {
  font-size: 11px;
  color: var(--color-text-muted);
}

.form-error {
  font-size: 12px;
  color: #f87171;
  padding: 4px 0;
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--color-border);
}

.btn {
  padding: 6px 16px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
}

.btn-cancel {
  background: none;
  color: var(--color-text-muted);
}

.btn-cancel:hover {
  background: var(--color-hover);
  color: var(--color-text);
}

.btn-save {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: #fff;
}

.btn-save:hover {
  background: #006cbd;
}
</style>
