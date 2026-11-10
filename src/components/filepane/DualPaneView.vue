<script setup lang="ts">
import { onMounted } from 'vue';
import FilePane from './FilePane.vue';
import { useLocalFilesStore } from '../../stores/localFiles';
import { useConnectionStore } from '../../stores/connection';
import { useFileActionsStore } from '../../stores/fileActions';
import type { PaneSide } from '../../types';

const localStore = useLocalFilesStore();
const connectionStore = useConnectionStore();
const actions = useFileActionsStore();

function activate(side: PaneSide) {
  actions.activePane = side;
}

function onDrop(from: PaneSide, paths: string[]) {
  const store = from === 'local' ? localStore : connectionStore;
  const wanted = new Set(paths);
  actions.transfer(from, store.files.filter((f) => wanted.has(f.path)));
}

onMounted(() => {
  localStore.initialize();
});
</script>

<template>
  <div class="dual-pane">
    <div class="pane-left">
      <FilePane
        title="Local"
        side="local"
        :path="localStore.currentPath"
        :files="localStore.files"
        v-model:selected="localStore.selected"
        :active="actions.activePane === 'local'"
        :loading="localStore.loading"
        @navigate="localStore.navigateTo"
        @navigate-up="localStore.navigateUp"
        @activate="activate('local')"
        @mkdir="actions.requestMkdir"
        @rename="actions.requestRename"
        @delete="actions.requestDelete"
        @transfer="actions.transfer('local')"
        @drop="onDrop"
      />
    </div>
    <div class="pane-divider" />
    <div class="pane-right">
      <template v-if="connectionStore.connected">
        <FilePane
          title="Distant"
          side="remote"
          :path="connectionStore.currentPath"
          :files="connectionStore.files"
          v-model:selected="connectionStore.selected"
          :active="actions.activePane === 'remote'"
          :loading="connectionStore.loading"
          @navigate="connectionStore.navigateTo"
          @navigate-up="connectionStore.navigateUp"
          @activate="activate('remote')"
          @mkdir="actions.requestMkdir"
          @rename="actions.requestRename"
          @delete="actions.requestDelete"
          @transfer="actions.transfer('remote')"
          @drop="onDrop"
        />
      </template>
      <div v-else class="pane-placeholder">
        <div v-if="connectionStore.error" class="placeholder-error" role="alert">
          <strong>Échec de la connexion</strong>
          <span>{{ connectionStore.error }}</span>
        </div>
        <span v-else-if="connectionStore.connecting" class="placeholder-text">Connexion en cours…</span>
        <span v-else class="placeholder-text">Non connecté — sélectionnez un profil et cliquez Connexion</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dual-pane {
  display: flex;
  height: 100%;
  overflow: hidden;
}

.pane-left,
.pane-right {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}

.pane-divider {
  width: 3px;
  background: var(--color-border);
  cursor: col-resize;
  flex-shrink: 0;
}

.pane-divider:hover {
  background: var(--color-accent);
}

.pane-placeholder {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 100%;
  background: var(--color-bg);
}

.placeholder-error {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-width: 80%;
  padding: 12px 16px;
  border: 1px solid var(--color-danger-border);
  border-radius: 4px;
  background: var(--bg-danger);
  color: var(--color-danger-text);
  font-size: 12px;
  word-break: break-word;
}

.placeholder-text {
  color: var(--color-text-muted);
  font-size: 13px;
}
</style>
