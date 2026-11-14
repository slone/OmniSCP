<script setup lang="ts">
import { useLocalFilesStore } from '../stores/localFiles';
import { t } from '../i18n';

const localStore = useLocalFilesStore();
</script>

<template>
  <div class="statusbar">
    <span class="status-item">{{ localStore.currentPath || t.statusBar.disconnected }}</span>
    <span class="status-separator" aria-hidden="true">|</span>
    <span class="status-item">{{ t.common.items(localStore.files.length) }}</span>
    <span class="status-spacer" />
    <span class="status-item">OmniSCP v0.1.0</span>
  </div>
</template>

<style scoped>
.statusbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 2px 8px;
  background: var(--color-surface);
  border-top: 1px solid var(--color-border);
  font-size: 11px;
  color: var(--color-text-muted);
  min-height: 22px;
}

.status-separator {
  color: var(--color-border);
}

.status-spacer {
  flex: 1;
}
</style>
