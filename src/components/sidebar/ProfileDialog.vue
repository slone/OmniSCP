<script setup lang="ts">
import { ref, watch, computed, onMounted, useId } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { Profile, ProtocolType, AuthMethod, ProbeResult } from '../../types';
import { errorMessage, isBackendError } from '../../utils/errors';
import BaseDialog from '../dialogs/BaseDialog.vue';
import SuggestInput from '../SuggestInput.vue';
import { t } from '../../i18n';

const props = defineProps<{
  profile?: Profile;
  /// Existing group names, offered as suggestions.
  groups?: string[];
}>();

const emit = defineEmits<{
  /// secret: string = store it, null = forget it, undefined = keep the saved one.
  save: [profile: Profile, secret: string | null | undefined];
  cancel: [];
}>();

const allProtocols: ProtocolType[] = ['Sftp', 'Scp', 'Ftp', 'Ftps', 'WebDav', 'WebDavs'];

const availableProtocols: ProtocolType[] = ['Sftp', 'Scp', 'Ftp', 'Ftps'];

const defaultPorts: Record<ProtocolType, number> = {
  Sftp: 22,
  Scp: 22,
  Ftp: 21,
  Ftps: 21,
  WebDav: 80,
  WebDavs: 443,
};

type AuthType = 'Password' | 'KeyFile' | 'KeyWithPassphrase' | 'Agent';
const allAuthTypes = computed<{ value: AuthType; label: string }[]>(() => [
  { value: 'Password', label: t.profile.authPassword },
  { value: 'KeyFile', label: t.profile.authKey },
  { value: 'KeyWithPassphrase', label: t.profile.authKeyPassphrase },
  { value: 'Agent', label: t.profile.authAgent },
]);

/// Ties each label to its field.
const ids = useId();
const fieldId = (name: string) => `${ids}-${name}`;

// A duplicate arrives without an id: it is saved as a new profile.
const isEditing = computed(() => !!props.profile?.id);

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
const localDirectory = ref(props.profile?.local_directory ?? '');

async function browseLocal() {
  const chosen = await open({ directory: true, defaultPath: localDirectory.value || undefined }).catch(() => null);
  if (typeof chosen === 'string') localDirectory.value = chosen;
}
const description = ref(props.profile?.description ?? '');
const tagsText = ref(props.profile?.tags.join(', ') ?? '');
const group = ref(props.profile?.group ?? '');

const implicitTls = ref(props.profile?.implicit_tls ?? false);
const isFtp = computed(() => protocol.value === 'Ftp' || protocol.value === 'Ftps');
/// FTP only knows passwords.
const authTypes = computed(() => (isFtp.value ? allAuthTypes.value.filter((a) => a.value === 'Password') : allAuthTypes.value));

const portManuallyChanged = ref(false);

function defaultPort() {
  return protocol.value === 'Ftps' && implicitTls.value ? 990 : defaultPorts[protocol.value];
}

watch([protocol, implicitTls], () => {
  if (!portManuallyChanged.value) {
    port.value = defaultPort();
  }
  if (isFtp.value) authType.value = 'Password';
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
const isPassword = computed(() => authType.value === 'Password');
const secretLabel = computed(() => (isPassword.value ? t.profile.password : t.profile.passphrase));
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

/// The form's error, or null when it describes a usable profile.
function checkForm(): string | null {
  if (!name.value.trim()) return t.profile.nameRequired;
  if (!host.value.trim()) return t.profile.hostRequired;
  if (!username.value.trim()) return t.profile.userRequired;
  if (!Number.isInteger(port.value) || port.value < 1 || port.value > 65535) {
    return t.profile.portRange;
  }
  if (showKeyPath.value && !keyPath.value.trim()) return t.profile.keyRequired;
  return null;
}

/// The profile described by the form, with `id` as its id.
function buildProfile(id: string, storeSecret: boolean): Profile {
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

  return {
    id,
    name: name.value.trim(),
    host: host.value.trim(),
    port: port.value,
    protocol: protocol.value,
    username: username.value.trim(),
    auth_method: authMethod,
    remote_directory: remoteDirectory.value.trim() || undefined,
    local_directory: localDirectory.value.trim() || undefined,
    description: description.value.trim() || undefined,
    tags,
    group: group.value.trim() || undefined,
    save_password: storeSecret,
    implicit_tls: protocol.value === 'Ftps' && implicitTls.value,
  };
}

async function onSave() {
  validationError.value = checkForm() ?? '';
  if (validationError.value) return;
  const storeSecret = needsSecret.value && savePassword.value;
  if (storeSecret && !secret.value && !canKeepSaved.value) {
    validationError.value = isPassword.value ? t.profile.enterPassword : t.profile.enterPassphrase;
    return;
  }

  const id = props.profile?.id || (await invoke<string>('generate_id'));
  const profile = buildProfile(id, storeSecret);

  // A new profile has nothing in the keychain to forget.
  const forget = isEditing.value ? null : undefined;
  const secretChange = !storeSecret ? forget : secret.value ? secret.value : undefined;
  emit('save', profile, secretChange);
}

// --- Connection test ---
type TestState =
  | { status: 'running' }
  | { status: 'ok'; result: ProbeResult }
  | { status: 'unknown-key'; fingerprint: string }
  | { status: 'failed'; message: string };

const test = ref<TestState | null>(null);
// Bumped by every test and edit, so a late answer about an older form is ignored.
let testRun = 0;

const canTest = computed(
  () => !needsSecret.value || secret.value !== '' || (savePassword.value && canKeepSaved.value),
);

async function onTest(trustedFingerprint?: string) {
  const formError = checkForm();
  if (formError) {
    test.value = { status: 'failed', message: formError };
    return;
  }
  const run = ++testRun;
  test.value = { status: 'running' };
  try {
    const result = await invoke<ProbeResult>('test_connection', {
      profile: buildProfile(props.profile?.id ?? '', false),
      password: needsSecret.value && secret.value ? secret.value : null,
      useSaved: savePassword.value && canKeepSaved.value,
      trustedFingerprint: trustedFingerprint ?? null,
    });
    if (run === testRun) test.value = { status: 'ok', result };
  } catch (err) {
    if (run !== testRun) return;
    test.value =
      isBackendError(err) && err.kind === 'HostKeyUnknown' && err.fingerprint
        ? { status: 'unknown-key', fingerprint: err.fingerprint }
        : isBackendError(err) && err.kind === 'HostKeyMismatch'
          ? {
              status: 'failed',
              message: t.profile.hostKeyChanged(err.fingerprint ?? ''),
            }
          : { status: 'failed', message: errorMessage(err) };
  }
}

// An answer about different settings would mislead.
watch([host, port, protocol, implicitTls, username, authType, keyPath, secret, remoteDirectory], () => {
  testRun++;
  test.value = null;
});
</script>

<template>
  <BaseDialog :title="isEditing ? t.profile.editTitle : t.profile.newTitle" :width="420" @cancel="emit('cancel')">
    <form class="profile-form" @submit.prevent="onSave">
      <div class="form-group">
        <label class="form-label" :for="fieldId('name')">{{ t.profile.name }}</label>
        <input :id="fieldId('name')" v-model="name" type="text" class="form-input" :placeholder="t.profile.namePlaceholder" />
      </div>

      <div class="form-row">
        <div class="form-group form-group-grow">
          <label class="form-label" :for="fieldId('host')">{{ t.profile.host }}</label>
          <input :id="fieldId('host')" v-model="host" type="text" class="form-input" placeholder="example.com" spellcheck="false" />
        </div>
        <div class="form-group form-group-small">
          <label class="form-label" :for="fieldId('port')">{{ t.profile.port }}</label>
          <input
            :id="fieldId('port')"
            v-model.number="port"
            type="number"
            class="form-input"
            min="1"
            max="65535"
            @input="onPortInput"
          />
        </div>
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('protocol')">{{ t.profile.protocol }}</label>
        <select
          :id="fieldId('protocol')"
          v-model="protocol"
          class="form-input"
          :aria-describedby="protocol === 'Scp' || protocol === 'Ftp' ? fieldId('protocol-hint') : undefined"
        >
          <option v-for="p in allProtocols" :key="p" :value="p" :disabled="!availableProtocols.includes(p)">
            {{ p }}{{ availableProtocols.includes(p) ? '' : t.profile.soon }}
          </option>
        </select>
        <span v-if="protocol === 'Scp'" :id="fieldId('protocol-hint')" class="form-hint">{{ t.profile.scpHint }}</span>
        <span v-else-if="protocol === 'Ftp'" :id="fieldId('protocol-hint')" class="form-hint form-warning">{{ t.profile.ftpHint }}</span>
      </div>

      <div v-if="protocol === 'Ftps'" class="form-group">
        <label class="checkbox-label">
          <input v-model="implicitTls" type="checkbox" :aria-describedby="fieldId('implicit-hint')" />
          {{ t.profile.implicitTls }}
        </label>
        <span :id="fieldId('implicit-hint')" class="form-hint">{{ t.profile.implicitTlsHint }}</span>
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('user')">{{ t.profile.username }}</label>
        <input :id="fieldId('user')" v-model="username" type="text" class="form-input" placeholder="user" spellcheck="false" />
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('auth')">{{ t.profile.auth }}</label>
        <select :id="fieldId('auth')" v-model="authType" class="form-input">
          <option v-for="a in authTypes" :key="a.value" :value="a.value">{{ a.label }}</option>
        </select>
      </div>

      <div v-if="showKeyPath" class="form-group">
        <label class="form-label" :for="fieldId('key')">{{ t.profile.keyPath }}</label>
        <input :id="fieldId('key')" v-model="keyPath" type="text" class="form-input" placeholder="~/.ssh/id_rsa" spellcheck="false" />
      </div>

      <div v-if="needsSecret" class="form-group">
        <label class="checkbox-label">
          <input v-model="savePassword" type="checkbox" />
          {{ isPassword ? t.profile.savePassword : t.profile.savePassphrase }}
        </label>
      </div>

      <div v-if="needsSecret" class="form-group">
        <label class="form-label" :for="fieldId('secret')">{{ secretLabel }}</label>
        <input
          :id="fieldId('secret')"
          v-model="secret"
          type="password"
          class="form-input"
          autocomplete="new-password"
          :aria-describedby="fieldId('secret-hint')"
          :placeholder="savePassword && canKeepSaved ? t.profile.savedPlaceholder : ''"
        />
        <span :id="fieldId('secret-hint')" class="form-hint">
          {{ savePassword ? t.profile.keptInKeychain : t.profile.testOnly }}
        </span>
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('dir')">{{ t.profile.remoteDirectory }}</label>
        <input :id="fieldId('dir')" v-model="remoteDirectory" type="text" class="form-input" placeholder="/var/www" spellcheck="false" />
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('local-dir')">{{ t.profile.localDirectory }}</label>
        <div class="input-with-button">
          <input
            :id="fieldId('local-dir')"
            v-model="localDirectory"
            type="text"
            class="form-input"
            spellcheck="false"
            :aria-describedby="fieldId('local-dir-hint')"
          />
          <button type="button" class="btn" @click="browseLocal">{{ t.profile.browse }}</button>
        </div>
        <span :id="fieldId('local-dir-hint')" class="form-hint">{{ t.profile.localDirectoryHint }}</span>
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('group')">{{ t.profile.group }}</label>
        <SuggestInput
          :id="fieldId('group')"
          v-model="group"
          :suggestions="groups ?? []"
          :placeholder="t.profile.groupPlaceholder"
        />
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('description')">{{ t.profile.description }}</label>
        <input
          :id="fieldId('description')"
          v-model="description"
          type="text"
          class="form-input"
          :placeholder="t.profile.descriptionPlaceholder"
        />
      </div>

      <div class="form-group">
        <label class="form-label" :for="fieldId('tags')">{{ t.profile.tags }}</label>
        <input :id="fieldId('tags')" v-model="tagsText" type="text" class="form-input" placeholder="prod, web, backup" />
      </div>

      <div v-if="validationError" class="form-error" role="alert">{{ validationError }}</div>
      <!-- Enter in a field saves, as the Enregistrer button does. -->
      <button type="submit" hidden />
    </form>

    <template v-if="test" #status>
      <div class="test-result" :class="test.status" role="status">
        <template v-if="test.status === 'running'">{{ t.profile.testing }}</template>
        <template v-else-if="test.status === 'ok' && !test.result.directory_error">
          {{ t.profile.testOk }} <span class="mono">{{ test.result.initial_path }}</span>
        </template>
        <template v-else-if="test.status === 'ok'">
          {{ t.profile.testDirFailed }}
          {{ test.result.directory_error ? errorMessage(test.result.directory_error) : '' }}
        </template>
        <template v-else-if="test.status === 'unknown-key'">
          {{ t.profile.testUnknownKey }}
          <div class="mono">{{ test.fingerprint }}</div>
          <button class="btn btn-trust" @click="onTest(test.fingerprint)">{{ t.profile.trustAndTest }}</button>
        </template>
        <template v-else>{{ test.message }}</template>
      </div>
    </template>

    <template #footer>
      <button
        class="btn btn-test"
        :disabled="test?.status === 'running' || !canTest"
        :title="canTest ? t.profile.testHint : isPassword ? t.profile.testNeedsPassword : t.profile.testNeedsPassphrase"
        @click="onTest()"
      >
        {{ t.profile.test }}
      </button>
      <button class="btn" @click="emit('cancel')">{{ t.common.cancel }}</button>
      <button class="btn btn-primary" @click="onSave">{{ t.common.save }}</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.profile-form {
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

.input-with-button {
  display: flex;
  gap: 6px;
}

.input-with-button .form-input {
  flex: 1;
  min-width: 0;
}

.form-group-grow {
  flex: 1;
}

.form-group-small {
  width: 80px;
  flex-shrink: 0;
}

.form-input option {
  background: var(--color-bg);
  color: var(--color-text);
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}

.form-hint {
  font-size: 11px;
  color: var(--color-text-muted);
}

.form-hint.form-warning {
  color: var(--color-warning);
}

.form-error {
  color: var(--color-danger);
  padding: 4px 0;
}

.btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.btn-test {
  margin-right: auto;
}

.test-result {
  padding: 8px;
  border-radius: 3px;
  background: var(--bg-neutral);
  overflow-wrap: anywhere;
}

.test-result.ok {
  background: var(--bg-success);
}

.test-result.unknown-key {
  background: var(--bg-warning);
}

.test-result.failed {
  background: var(--bg-danger);
  color: var(--color-danger-text);
}

.mono {
  font-family: var(--font-mono);
}

.btn-trust {
  margin-top: 6px;
}
</style>
