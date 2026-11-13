<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import BaseDialog from './BaseDialog.vue';
import type { FileEntry, PaneSide, TreeSize } from '../../types';
import { formatSize } from '../../utils/format';
import { errorMessage } from '../../utils/errors';
import { useFileActionsStore } from '../../stores/fileActions';
import { useConnectionStore } from '../../stores/connection';

const props = defineProps<{ side: PaneSide; entries: FileEntry[] }>();
const emit = defineEmits<{ close: [] }>();

const actions = useFileActionsStore();

const single = computed(() => (props.entries.length === 1 ? props.entries[0] : null));
const hasFolders = computed(() => props.entries.some((e) => e.is_dir && !e.is_symlink));
const filesSize = computed(() => props.entries.reduce((sum, e) => (e.is_dir ? sum : sum + e.size), 0));

function bytes(n: number) {
  return `${n.toLocaleString('fr-FR')} octet${n > 1 ? 's' : ''}`;
}

function typeLabel(e: FileEntry) {
  if (e.is_symlink) return e.is_dir ? 'Lien vers un dossier' : 'Lien vers un fichier';
  return e.is_dir ? 'Dossier' : 'Fichier';
}

function location(e: FileEntry) {
  const parent = e.path.slice(0, e.path.length - e.name.length).replace(/[\\/]$/, '');
  return parent || '/';
}

function account(name?: string, id?: number) {
  if (name && id !== undefined) return `${name} (${id})`;
  return name ?? (id !== undefined ? String(id) : '—');
}

function fullDate(timestamp?: number) {
  return timestamp ? new Date(timestamp * 1000).toLocaleString('fr-FR') : '—';
}

// --- Link target ---
const linkTarget = ref<string | null>(null);
const linkError = ref<string | null>(null);

onMounted(async () => {
  const e = single.value;
  if (!e?.is_symlink) return;
  try {
    linkTarget.value = await invoke<string>(props.side === 'local' ? 'local_link_target' : 'remote_link_target', {
      path: e.path,
    });
  } catch (err) {
    linkError.value = errorMessage(err);
  }
});

// --- Folder size ---
const size = ref<TreeSize | null>(null);
const computing = ref(false);
const sizeError = ref<string | null>(null);

async function computeSize() {
  computing.value = true;
  sizeError.value = null;
  try {
    size.value = await invoke<TreeSize>(props.side === 'local' ? 'local_folder_size' : 'remote_folder_size', {
      paths: props.entries.map((e) => e.path),
    });
  } catch (err) {
    sizeError.value = errorMessage(err);
    await useConnectionStore().handleConnectionLost(err);
  } finally {
    computing.value = false;
  }
}

function cancelSize() {
  invoke('cancel_folder_size').catch(() => {});
}

onBeforeUnmount(() => {
  if (computing.value) cancelSize();
});

// --- Permissions ---
const WHO = [
  { label: 'Propriétaire', shift: 6 },
  { label: 'Groupe', shift: 3 },
  { label: 'Autres', shift: 0 },
];
const WHAT = [
  { label: 'Lecture', bit: 4 },
  { label: 'Écriture', bit: 2 },
  { label: 'Exécution', bit: 1 },
];
const SPECIAL = [
  { label: 'Set-UID', bit: 0o4000, hint: 'Exécuté avec les droits du propriétaire' },
  { label: 'Set-GID', bit: 0o2000, hint: 'Exécuté avec les droits du groupe ; dans un dossier, le contenu hérite du groupe' },
  { label: 'Sticky', bit: 0o1000, hint: 'Dans un dossier, seul le propriétaire d’un élément peut le supprimer ou le renommer' },
];

const original = computed(() => single.value?.mode);
const mode = ref(original.value ?? 0);
const octal = ref(toOctal(mode.value));
const applying = ref(false);
const applyFailed = ref(false);

function toOctal(m: number) {
  return m.toString(8).padStart(4, '0');
}

watch(mode, (m) => {
  if (parseInt(octal.value, 8) !== m) octal.value = toOctal(m);
});

const octalValid = computed(() => /^[0-7]{3,4}$/.test(octal.value));

function onOctalInput() {
  if (octalValid.value) mode.value = parseInt(octal.value, 8);
}

function hasBit(bit: number) {
  return (mode.value & bit) !== 0;
}

function toggle(bit: number) {
  mode.value ^= bit;
}

const changed = computed(() => original.value !== undefined && mode.value !== original.value);

async function applyPermissions() {
  const e = single.value;
  if (!e || !changed.value || !octalValid.value) return;
  applying.value = true;
  applyFailed.value = false;
  const ok = await actions.setPermissions(props.side, e, mode.value);
  applying.value = false;
  if (ok) emit('close');
  else applyFailed.value = true;
}
</script>

<template>
  <BaseDialog :title="single ? `Propriétés — ${single.name}` : `Propriétés — ${entries.length} éléments`" :width="460" @cancel="emit('close')">
    <dl class="facts">
      <template v-if="single">
        <dt>Type</dt>
        <dd>{{ typeLabel(single) }}</dd>
        <template v-if="single.is_symlink">
          <dt>Cible</dt>
          <dd class="mono">
            <span v-if="linkTarget !== null">{{ linkTarget }}</span>
            <span v-else-if="linkError" class="error">{{ linkError }}</span>
            <span v-else class="muted">…</span>
          </dd>
        </template>
        <dt>Emplacement</dt>
        <dd class="mono">{{ location(single) }}</dd>
        <dt>Modifié</dt>
        <dd>{{ fullDate(single.modified) }}</dd>
        <dt>Propriétaire</dt>
        <dd>{{ account(single.owner, single.uid) }}</dd>
        <dt>Groupe</dt>
        <dd>{{ account(single.group, single.gid) }}</dd>
      </template>
      <template v-else>
        <dt>Sélection</dt>
        <dd>
          {{ entries.filter((e) => e.is_dir).length }} dossier(s), {{ entries.filter((e) => !e.is_dir).length }} fichier(s)
        </dd>
      </template>

      <dt>Taille</dt>
      <dd>
        <template v-if="!hasFolders && !size">
          {{ formatSize(filesSize) }} <span class="muted">({{ bytes(filesSize) }})</span>
        </template>
        <template v-else-if="size">
          {{ formatSize(size.bytes) }} <span class="muted">({{ bytes(size.bytes) }})</span>
          <div class="muted">{{ size.files }} fichier(s) dans {{ size.folders }} dossier(s)</div>
          <div v-if="size.unreadable > 0" class="warning">
            {{ size.unreadable }} élément(s) illisible(s) : la taille réelle est plus grande.
          </div>
        </template>
        <template v-else-if="computing">
          <span class="muted">Calcul en cours…</span>
          <button class="link-btn" @click="cancelSize">Arrêter</button>
        </template>
        <template v-else>
          <button class="link-btn" @click="computeSize">Calculer</button>
          <span class="muted"> — parcourt tout le contenu{{ side === 'remote' ? ', sur une connexion séparée' : '' }}</span>
          <div v-if="sizeError" class="error">{{ sizeError }}</div>
        </template>
      </dd>
    </dl>

    <section v-if="single" class="perms">
      <h3 class="form-label">Droits</h3>
      <template v-if="original !== undefined">
        <table class="perm-grid">
          <thead>
            <tr>
              <th></th>
              <th v-for="what in WHAT" :key="what.bit">{{ what.label }}</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="who in WHO" :key="who.shift">
              <th scope="row">{{ who.label }}</th>
              <td v-for="what in WHAT" :key="what.bit">
                <input
                  type="checkbox"
                  :checked="hasBit(what.bit << who.shift)"
                  :aria-label="`${who.label} — ${what.label}`"
                  @change="toggle(what.bit << who.shift)"
                />
              </td>
            </tr>
          </tbody>
        </table>
        <div class="special">
          <label v-for="s in SPECIAL" :key="s.bit" :title="s.hint">
            <input type="checkbox" :checked="hasBit(s.bit)" @change="toggle(s.bit)" />
            {{ s.label }}
          </label>
        </div>
        <label class="octal">
          Octal
          <input
            v-model="octal"
            class="form-input mono"
            maxlength="4"
            spellcheck="false"
            :aria-invalid="!octalValid"
            @input="onOctalInput"
          />
          <span v-if="!octalValid" class="error">3 ou 4 chiffres de 0 à 7</span>
        </label>
        <p v-if="single.is_symlink" class="muted">S’applique à la cible du lien.</p>
        <p v-if="applyFailed" class="error" role="alert">Les droits n’ont pas pu être modifiés. Le détail est dans le journal.</p>
      </template>
      <p v-else class="muted">
        {{ single.permissions ?? '—' }} — les droits Unix n’existent pas sur ce système.
      </p>
    </section>

    <template #footer>
      <button class="btn" @click="emit('close')">{{ changed ? 'Annuler' : 'Fermer' }}</button>
      <button
        v-if="single && original !== undefined"
        class="btn btn-primary"
        :disabled="!changed || !octalValid || applying"
        @click="applyPermissions"
      >
        Appliquer les droits
      </button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.facts {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 6px 12px;
  margin: 0;
}

.facts dt {
  color: var(--color-text-muted);
}

.facts dd {
  margin: 0;
  min-width: 0;
  word-break: break-all;
}

.mono {
  font-family: var(--font-mono);
}

.muted {
  color: var(--color-text-muted);
}

.warning {
  color: var(--color-warning);
}

.error {
  color: var(--color-danger);
}

.link-btn {
  padding: 0;
  border: none;
  background: none;
  color: var(--color-accent);
  font: inherit;
  cursor: pointer;
  text-decoration: underline;
}

.perms {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding-top: 8px;
  border-top: 1px solid var(--color-border);
}

.perms h3 {
  margin: 0;
}

.perm-grid {
  border-collapse: collapse;
}

.perm-grid th,
.perm-grid td {
  padding: 2px 10px 2px 0;
  font-weight: normal;
  text-align: left;
}

.perm-grid thead th {
  color: var(--color-text-muted);
  font-size: 11px;
}

.perm-grid td {
  text-align: center;
}

.special {
  display: flex;
  gap: 14px;
}

.special label,
.octal {
  display: flex;
  align-items: center;
  gap: 6px;
}

.octal .form-input {
  width: 70px;
}
</style>
