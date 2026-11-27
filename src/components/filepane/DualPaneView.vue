<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';
import { getCurrentWebview } from '@tauri-apps/api/webview';
import FilePane from './FilePane.vue';
import { useLocalFilesStore } from '../../stores/localFiles';
import { useConnectionStore } from '../../stores/connection';
import { useFileActionsStore } from '../../stores/fileActions';
import type { PaneSide } from '../../types';
import { t } from '../../i18n';
import { baseName, dragState, paneAt } from '../../utils/paneDrag';
import Splitter from '../Splitter.vue';
import { layout, saveLayout, PANE_SPLIT } from '../../utils/layout';

const container = ref<HTMLElement | null>(null);
/// A pixel of mouse movement, as a share of the two panes' room.
const splitScale = () => 1 / Math.max(1, (container.value?.clientWidth ?? 1) - 7);
const splitText = (share: number) => t.layout.share(Math.round(share * 100));

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

/// The pane that would receive what is being dragged, if any.
const dropTarget = computed<PaneSide | null>(() => {
  const over = dragState.over;
  if (!over) return null;
  if (dragState.drag) return over !== dragState.drag.from ? over : null;
  // From the file manager: only an upload makes sense.
  return over === 'remote' && connectionStore.connected ? 'remote' : null;
});

/// Files and folders dropped from the system's file manager are uploaded to
/// the remote folder. Tauri hands over their paths and keeps the webview from
/// opening them itself.
let stopListening: (() => void) | undefined;
const IS_WINDOWS = navigator.userAgent.includes('Windows');

async function listenForSystemDrops() {
  stopListening = await getCurrentWebview().onDragDropEvent(({ payload }) => {
    if (payload.type === 'leave') {
      dragState.over = null;
      return;
    }
    // Tauri calls the position physical, but only Windows gives device
    // pixels; macOS and Linux already give CSS pixels.
    const scale = IS_WINDOWS ? window.devicePixelRatio || 1 : 1;
    dragState.over = paneAt(payload.position.x / scale, payload.position.y / scale);
    if (payload.type !== 'drop') return;
    const target = dropTarget.value;
    dragState.over = null;
    if (target === 'remote') {
      actions.transfer('local', payload.paths.map((path) => ({ path, name: baseName(path) })));
    }
  });
}

onMounted(() => {
  localStore.initialize();
  listenForSystemDrops().catch(() => {
    // Outside Tauri (the frontend alone in a browser) there is nothing to listen to.
  });
});

onUnmounted(() => stopListening?.());
</script>

<template>
  <div ref="container" class="dual-pane">
    <div class="pane-left" :style="{ flexGrow: layout.paneSplit }">
      <FilePane
        :title="t.panes.local"
        side="local"
        :path="localStore.currentPath"
        :files="localStore.files"
        v-model:selected="localStore.selected"
        :active="actions.activePane === 'local'"
        :loading="localStore.loading"
        :can-go-back="localStore.canGoBack"
        :error="localStore.error"
        :can-go-forward="localStore.canGoForward"
        :drop-highlight="dropTarget === 'local'"
        @navigate="localStore.navigateTo"
        @navigate-up="localStore.navigateUp"
        @back="localStore.goBack"
        @forward="localStore.goForward"
        @refresh="localStore.refresh"
        @properties="actions.requestProperties"
        @activate="activate('local')"
        @mkdir="actions.requestMkdir"
        @rename="actions.requestRename"
        @delete="actions.requestDelete"
        @transfer="actions.transfer('local')"
        @move="actions.requestMove('local')"
        @drop="onDrop"
      />
    </div>
    <Splitter
      v-model="layout.paneSplit"
      :min="PANE_SPLIT.min"
      :max="PANE_SPLIT.max"
      :step="0.02"
      :scale="splitScale"
      :reset="PANE_SPLIT.default"
      :label="t.layout.panes"
      :value-text="splitText"
      @commit="saveLayout"
    />
    <div class="pane-right" :style="{ flexGrow: 1 - layout.paneSplit }">
      <template v-if="connectionStore.connected">
        <FilePane
          :title="t.panes.remote"
          side="remote"
          :path="connectionStore.currentPath"
          :files="connectionStore.files"
          v-model:selected="connectionStore.selected"
          :active="actions.activePane === 'remote'"
          :loading="connectionStore.loading"
          :can-go-back="connectionStore.canGoBack"
          :error="connectionStore.error"
          :can-go-forward="connectionStore.canGoForward"
          :drop-highlight="dropTarget === 'remote'"
          @navigate="connectionStore.navigateTo"
          @navigate-up="connectionStore.navigateUp"
          @back="connectionStore.goBack"
          @forward="connectionStore.goForward"
          @refresh="connectionStore.refresh"
          @properties="actions.requestProperties"
          @activate="activate('remote')"
          @mkdir="actions.requestMkdir"
          @rename="actions.requestRename"
          @delete="actions.requestDelete"
          @transfer="actions.transfer('remote')"
          @move="actions.requestMove('remote')"
          @edit="actions.requestEdit"
          @drop="onDrop"
        />
      </template>
      <div v-else class="pane-placeholder">
        <div v-if="connectionStore.error" class="placeholder-error" role="alert">
          <strong>{{ t.panes.connectFailed }}</strong>
          <span>{{ connectionStore.error }}</span>
        </div>
        <span v-else-if="connectionStore.connecting" class="placeholder-text" role="status">{{ t.panes.connecting }}</span>
        <span v-else class="placeholder-text">{{ t.panes.notConnected }}</span>
      </div>
    </div>
    <div
      v-if="dragState.drag"
      :class="['drag-label', { refused: !dropTarget }]"
      :style="{ transform: `translate(${dragState.drag.x + 14}px, ${dragState.drag.y + 10}px)` }"
      aria-hidden="true"
    >
      {{ dragState.drag.label }}
    </div>
  </div>
</template>

<style scoped>
.drag-label {
  position: fixed;
  top: 0;
  left: 0;
  z-index: 1000;
  max-width: 280px;
  padding: 3px 8px;
  overflow: hidden;
  border-radius: 4px;
  background: var(--color-accent);
  color: #fff;
  font-size: 12px;
  white-space: nowrap;
  text-overflow: ellipsis;
  pointer-events: none;
}

.drag-label.refused {
  opacity: 0.6;
}

.dual-pane {
  display: flex;
  height: 100%;
  overflow: hidden;
}

.pane-left,
.pane-right {
  flex-basis: 0;
  min-width: 0;
  overflow: hidden;
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
