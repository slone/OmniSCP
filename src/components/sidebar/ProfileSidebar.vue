<script setup lang="ts">
import { ref, computed, onMounted, nextTick } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import type { Profile } from '../../types';
import { useProfilesStore } from '../../stores/profiles';
import { useLogStore } from '../../stores/logs';
import { errorMessage } from '../../utils/errors';
import ProfileDialog from './ProfileDialog.vue';
import ProfileExchangeDialog from './ProfileExchangeDialog.vue';
import BaseDialog from '../dialogs/BaseDialog.vue';
import InputDialog from '../dialogs/InputDialog.vue';

const profileStore = useProfilesStore();

const showDialog = ref(false);
const editingProfile = ref<Profile | undefined>(undefined);
const search = ref('');

const FILE_FILTERS = [{ name: 'Profils OmniSCP', extensions: ['json'] }];
const COLLAPSED_KEY = 'omniscp.collapsedGroups';

const visibleProfiles = computed(() => {
  const words = search.value.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return profileStore.profiles;
  return profileStore.profiles.filter((p) => {
    const text = [p.name, p.host, p.username, p.protocol, p.description ?? '', p.group ?? '', ...p.tags]
      .join(' ')
      .toLowerCase();
    return words.every((w) => text.includes(w));
  });
});

// --- Groups ---
function readCollapsed(): string[] {
  try {
    const value = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]');
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}

const collapsed = ref(new Set<string>(readCollapsed()));

function toggleGroup(name: string) {
  const next = new Set(collapsed.value);
  if (next.has(name)) next.delete(name);
  else next.add(name);
  collapsed.value = next;
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
  } catch {
    // Only a convenience: the groups open again next time.
  }
}

/// Groups first, then ungrouped profiles at the top level, as folders before files.
const sections = computed(() =>
  profileStore.groups
    .map((name) => ({ name, items: visibleProfiles.value.filter((p) => p.group === name) }))
    .filter((g) => g.items.length > 0),
);
const ungrouped = computed(() => visibleProfiles.value.filter((p) => !p.group));

function isOpen(name: string) {
  // While searching, matches are never hidden in a closed group.
  return search.value.trim() !== '' || !collapsed.value.has(name);
}

const renamingGroup = ref<string | null>(null);

function onRenameGroup(to: string) {
  const from = renamingGroup.value;
  renamingGroup.value = null;
  if (from === null) return;
  if (collapsed.value.has(from)) {
    toggleGroup(from);
    if (!collapsed.value.has(to)) toggleGroup(to);
  }
  profileStore.renameGroup(from, to);
}

onMounted(() => {
  profileStore.loadProfiles();
});

function openCreateDialog() {
  editingProfile.value = undefined;
  showDialog.value = true;
}

function openEditDialog(profile: Profile) {
  editingProfile.value = { ...profile };
  showDialog.value = true;
}

/// Opens the editor on a copy; the password is not copied, it stays with the original.
function openDuplicateDialog(profile: Profile) {
  editingProfile.value = {
    ...profile,
    tags: [...profile.tags],
    id: '',
    name: `${profile.name} (copie)`,
    save_password: false,
  };
  showDialog.value = true;
}

function onDialogSave(profile: Profile, secret: string | null | undefined) {
  profileStore.saveProfile(profile, secret);
  showDialog.value = false;
}

function onDialogCancel() {
  showDialog.value = false;
}

const deleting = ref<Profile | null>(null);
const keepRef = ref<HTMLButtonElement | null>(null);

async function onDeleteProfile(profile: Profile) {
  deleting.value = profile;
  await nextTick();
  // Focus the safe choice so Enter does not delete by accident.
  keepRef.value?.focus();
}

function confirmDelete() {
  const profile = deleting.value;
  deleting.value = null;
  if (profile) profileStore.deleteProfile(profile.id);
}

function onSelectProfile(id: string) {
  profileStore.selectProfile(profileStore.selectedId === id ? null : id);
}

// --- Import / export ---
const exchange = ref<{ mode: 'import' | 'export'; profiles: Profile[]; onlyGroup?: string } | null>(null);
const notice = ref<string | null>(null);

function openExport(onlyGroup?: string) {
  exchange.value = { mode: 'export', profiles: profileStore.profiles, onlyGroup };
}

async function openImport() {
  let path: string | null;
  try {
    path = await open({ multiple: false, directory: false, filters: FILE_FILTERS });
  } catch (err) {
    notice.value = errorMessage(err);
    return;
  }
  if (!path) return;
  try {
    const profiles = await invoke<Profile[]>('import_profiles', { path });
    if (profiles.length === 0) notice.value = 'Ce fichier ne contient aucun profil.';
    else exchange.value = { mode: 'import', profiles };
  } catch (err) {
    const message = errorMessage(err);
    useLogStore().addLog('Error', 'Profils', 'Échec', `Import — ${message}`);
    notice.value = message;
  }
}

async function onExchangeConfirm(list: Profile[]) {
  const mode = exchange.value?.mode;
  exchange.value = null;
  if (mode === 'import') {
    await profileStore.addProfiles(list);
    return;
  }
  let path: string | null;
  try {
    path = await save({ defaultPath: 'profils-omniscp.json', filters: FILE_FILTERS });
  } catch (err) {
    notice.value = errorMessage(err);
    return;
  }
  if (path && !(await profileStore.exportProfiles(list, path))) {
    notice.value = "L'export a échoué. Le détail est dans le journal.";
  }
}
</script>

<template>
  <aside class="sidebar">
    <div class="sidebar-header">
      <span class="sidebar-title">Profils</span>
      <input
        v-if="profileStore.profiles.length > 0"
        v-model="search"
        type="search"
        class="search-input"
        placeholder="Rechercher…"
        title="Nom, hôte, utilisateur, groupe, tag ou description"
        aria-label="Rechercher un profil"
        spellcheck="false"
        @keydown.esc="search = ''"
      />
    </div>

    <div class="sidebar-content">
      <div v-if="profileStore.profiles.length === 0" class="empty-state">
        (aucun profil)
      </div>
      <div v-else-if="visibleProfiles.length === 0" class="empty-state">
        Aucun profil ne correspond
      </div>
      <ul v-else class="profile-list">
        <li v-for="group in sections" :key="'g:' + group.name" class="group">
          <div class="group-header">
            <button
              class="group-toggle"
              :aria-expanded="isOpen(group.name)"
              @click="toggleGroup(group.name)"
            >
              <span class="chevron">{{ isOpen(group.name) ? '▾' : '▸' }}</span>
              <span class="group-name">{{ group.name }}</span>
              <span class="group-count">{{ group.items.length }}</span>
            </button>
            <div class="profile-actions">
              <button class="btn-icon" title="Renommer le groupe" @click="renamingGroup = group.name">&#9998;</button>
              <button class="btn-icon" title="Exporter ce groupe" @click="openExport(group.name)">&#8682;</button>
            </div>
          </div>
          <ul v-if="isOpen(group.name)" class="profile-list nested">
            <li
              v-for="profile in group.items"
              :key="profile.id"
              class="profile-item"
              :class="{ active: profileStore.selectedId === profile.id }"
              @click="onSelectProfile(profile.id)"
            >
              <div class="profile-info">
                <div class="profile-name">{{ profile.name }}</div>
                <div class="profile-detail">{{ profile.protocol }} · {{ profile.host }}</div>
                <div v-if="profile.tags.length > 0" class="profile-tags">
                  <span v-for="tag in profile.tags" :key="tag" class="tag">{{ tag }}</span>
                </div>
              </div>
              <div class="profile-actions">
                <button class="btn-icon" title="Modifier" @click.stop="openEditDialog(profile)">&#9998;</button>
                <button class="btn-icon" title="Dupliquer" @click.stop="openDuplicateDialog(profile)">&#10697;</button>
                <button class="btn-icon btn-icon-danger" title="Supprimer" @click.stop="onDeleteProfile(profile)">&#128465;</button>
              </div>
            </li>
          </ul>
        </li>
        <li
          v-for="profile in ungrouped"
          :key="profile.id"
          class="profile-item"
          :class="{ active: profileStore.selectedId === profile.id }"
          @click="onSelectProfile(profile.id)"
        >
          <div class="profile-info">
            <div class="profile-name">{{ profile.name }}</div>
            <div class="profile-detail">{{ profile.protocol }} · {{ profile.host }}</div>
            <div v-if="profile.tags.length > 0" class="profile-tags">
              <span v-for="tag in profile.tags" :key="tag" class="tag">{{ tag }}</span>
            </div>
          </div>
          <div class="profile-actions">
            <button class="btn-icon" title="Modifier" @click.stop="openEditDialog(profile)">&#9998;</button>
            <button class="btn-icon" title="Dupliquer" @click.stop="openDuplicateDialog(profile)">&#10697;</button>
            <button class="btn-icon btn-icon-danger" title="Supprimer" @click.stop="onDeleteProfile(profile)">&#128465;</button>
          </div>
        </li>
      </ul>
    </div>

    <div class="sidebar-footer">
      <button class="btn-add-profile" @click="openCreateDialog">+ Nouveau profil</button>
      <div class="exchange">
        <button class="btn-small" title="Ajouter des profils depuis un fichier" @click="openImport">Importer…</button>
        <button
          class="btn-small"
          title="Enregistrer des profils dans un fichier, sans mot de passe"
          :disabled="profileStore.profiles.length === 0"
          @click="openExport()"
        >Exporter…</button>
      </div>
    </div>

    <ProfileDialog
      v-if="showDialog"
      :profile="editingProfile"
      :groups="profileStore.groups"
      @save="onDialogSave"
      @cancel="onDialogCancel"
    />

    <ProfileExchangeDialog
      v-if="exchange"
      :mode="exchange.mode"
      :profiles="exchange.profiles"
      :only-group="exchange.onlyGroup"
      :existing="profileStore.profiles"
      :groups="profileStore.groups"
      @confirm="onExchangeConfirm"
      @cancel="exchange = null"
    />

    <InputDialog
      v-if="renamingGroup !== null"
      title="Renommer le groupe"
      label="Nouveau nom (un groupe existant les réunit)"
      :initial-value="renamingGroup"
      confirm-label="Renommer"
      select-all
      @submit="onRenameGroup"
      @cancel="renamingGroup = null"
    />

    <BaseDialog v-if="deleting" title="Supprimer le profil" :width="380" @cancel="deleting = null">
      <p class="dialog-text">Supprimer le profil « {{ deleting.name }} » ?</p>
      <p v-if="deleting.save_password" class="dialog-text muted">Son mot de passe enregistré sera aussi effacé du trousseau.</p>
      <template #footer>
        <button ref="keepRef" class="btn" @click="deleting = null">Annuler</button>
        <button class="btn btn-danger" @click="confirmDelete">Supprimer</button>
      </template>
    </BaseDialog>

    <BaseDialog v-if="notice" title="Profils" :width="400" @cancel="notice = null">
      <p class="dialog-text" role="alert">{{ notice }}</p>
      <template #footer>
        <button class="btn btn-primary" @click="notice = null">OK</button>
      </template>
    </BaseDialog>
  </aside>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: var(--color-surface);
  border-right: 1px solid var(--color-border);
  overflow: hidden;
}

.sidebar-header {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 12px;
  border-bottom: 1px solid var(--color-border);
}

.search-input {
  width: 100%;
  padding: 3px 6px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 12px;
}

.profile-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 3px;
  margin-top: 3px;
}

.tag {
  padding: 0 5px;
  border-radius: 3px;
  background: var(--color-active);
  color: var(--color-text-muted);
  font-size: 10px;
}

.sidebar-title {
  font-size: 12px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: 0.5px;
  color: var(--color-text-muted);
}

.sidebar-content {
  flex: 1;
  overflow-y: auto;
}

.empty-state {
  padding: 16px 12px;
  font-size: 12px;
  color: var(--color-text-muted);
  text-align: center;
  font-style: italic;
}

.profile-list {
  list-style: none;
  margin: 0;
  padding: 4px 0;
}

.profile-item {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  cursor: pointer;
  border-left: 3px solid transparent;
}

.profile-item:hover {
  background: var(--color-hover);
}

.profile-item.active {
  border-left-color: var(--color-accent);
  background: var(--color-hover);
}

.profile-info {
  flex: 1;
  min-width: 0;
}

.profile-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--color-text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.profile-detail {
  font-size: 11px;
  color: var(--color-text-muted);
  margin-top: 2px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.profile-actions {
  display: none;
  gap: 2px;
  flex-shrink: 0;
}

.profile-item:hover .profile-actions {
  display: flex;
}

.btn-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border: none;
  border-radius: 3px;
  background: none;
  color: var(--color-text-muted);
  font-size: 12px;
  cursor: pointer;
}

.btn-icon:hover {
  background: var(--color-active);
  color: var(--color-text);
}

.btn-icon-danger:hover {
  color: var(--color-danger);
}

.sidebar-footer {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px;
  border-top: 1px solid var(--color-border);
}

.exchange {
  display: flex;
  gap: 6px;
}

.btn-small {
  flex: 1;
  padding: 3px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: none;
  color: var(--color-text-muted);
  font-size: 11px;
  cursor: pointer;
}

.btn-small:hover:not(:disabled) {
  background: var(--color-hover);
  color: var(--color-text);
}

.btn-small:disabled {
  opacity: 0.5;
  cursor: default;
}

.group-header {
  display: flex;
  align-items: center;
  padding-right: 12px;
}

.group-header:hover {
  background: var(--color-hover);
}

.group-header:hover .profile-actions {
  display: flex;
}

.group-toggle {
  display: flex;
  flex: 1;
  min-width: 0;
  align-items: center;
  gap: 4px;
  padding: 6px 12px;
  border: none;
  background: none;
  color: var(--color-text);
  font: inherit;
  font-size: 12px;
  font-weight: 600;
  text-align: left;
  cursor: pointer;
}

.chevron {
  width: 10px;
  color: var(--color-text-muted);
}

.group-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.group-count {
  color: var(--color-text-muted);
  font-weight: normal;
  font-size: 11px;
}

.profile-list.nested {
  padding: 0;
}

.profile-list.nested .profile-item {
  padding-left: 26px;
}

.dialog-text {
  margin: 0;
}

.muted {
  color: var(--color-text-muted);
}

.btn-add-profile {
  width: 100%;
  padding: 6px;
  border: 1px dashed var(--color-border);
  border-radius: 3px;
  background: none;
  color: var(--color-text-muted);
  font-size: 12px;
  cursor: pointer;
}

.btn-add-profile:hover {
  background: var(--color-hover);
  color: var(--color-text);
  border-color: var(--color-accent);
}
</style>
