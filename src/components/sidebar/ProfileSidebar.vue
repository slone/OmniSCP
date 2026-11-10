<script setup lang="ts">
import { ref, computed, onMounted } from 'vue';
import type { Profile } from '../../types';
import { useProfilesStore } from '../../stores/profiles';
import ProfileDialog from './ProfileDialog.vue';

const profileStore = useProfilesStore();

const showDialog = ref(false);
const editingProfile = ref<Profile | undefined>(undefined);
const search = ref('');

const visibleProfiles = computed(() => {
  const words = search.value.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return profileStore.profiles;
  return profileStore.profiles.filter((p) => {
    const text = [p.name, p.host, p.username, p.protocol, p.description ?? '', ...p.tags]
      .join(' ')
      .toLowerCase();
    return words.every((w) => text.includes(w));
  });
});

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

function onDeleteProfile(profile: Profile) {
  if (window.confirm(`Supprimer le profil « ${profile.name} » ?`)) {
    profileStore.deleteProfile(profile.id);
  }
}

function onSelectProfile(id: string) {
  profileStore.selectProfile(profileStore.selectedId === id ? null : id);
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
        placeholder="Rechercher (nom, hôte, tag…)"
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
        <li
          v-for="profile in visibleProfiles"
          :key="profile.id"
          class="profile-item"
          :class="{ active: profileStore.selectedId === profile.id }"
          @click="onSelectProfile(profile.id)"
        >
          <div class="profile-info">
            <div class="profile-name">{{ profile.name }}</div>
            <div class="profile-detail">
              {{ profile.protocol }} · {{ profile.host }}
            </div>
            <div v-if="profile.tags.length > 0" class="profile-tags">
              <span v-for="tag in profile.tags" :key="tag" class="tag">{{ tag }}</span>
            </div>
          </div>
          <div class="profile-actions">
            <button
              class="btn-icon"
              title="Modifier"
              @click.stop="openEditDialog(profile)"
            >&#9998;</button>
            <button
              class="btn-icon"
              title="Dupliquer"
              @click.stop="openDuplicateDialog(profile)"
            >&#10697;</button>
            <button
              class="btn-icon btn-icon-danger"
              title="Supprimer"
              @click.stop="onDeleteProfile(profile)"
            >&#128465;</button>
          </div>
        </li>
      </ul>
    </div>

    <div class="sidebar-footer">
      <button class="btn-add-profile" @click="openCreateDialog">+ Nouveau profil</button>
    </div>

    <ProfileDialog
      v-if="showDialog"
      :profile="editingProfile"
      @save="onDialogSave"
      @cancel="onDialogCancel"
    />
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
  padding: 8px;
  border-top: 1px solid var(--color-border);
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
