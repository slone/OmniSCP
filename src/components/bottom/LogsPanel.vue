<script setup lang="ts">
import { useLogStore } from '../../stores/logs';

const logStore = useLogStore();

function formatTime(timestamp: number): string {
  return new Date(timestamp * 1000).toLocaleTimeString('fr-FR', {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}
</script>

<template>
  <div class="logs-panel">
    <div class="log-entries">
      <div
        v-for="(entry, i) in logStore.entries"
        :key="i"
        :class="['log-entry', 'log-' + entry.level.toLowerCase()]"
      >
        <span class="log-time">{{ formatTime(entry.timestamp) }}</span>
        <span :class="['log-level', 'level-' + entry.level.toLowerCase()]">
          {{ entry.level }}
        </span>
        <span class="log-action">{{ entry.action }}</span>
        <span class="log-status">{{ entry.status }}</span>
        <span v-if="entry.detail" class="log-detail">{{ entry.detail }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.logs-panel {
  height: 100%;
  overflow-y: auto;
  font-family: var(--font-mono);
  font-size: 11px;
}

.log-entry {
  display: flex;
  gap: 8px;
  padding: 2px 8px;
  align-items: baseline;
}

.log-entry:hover {
  background: var(--color-hover);
}

.log-time {
  color: var(--color-text-muted);
  flex-shrink: 0;
}

.log-level {
  flex-shrink: 0;
  width: 55px;
  font-weight: 600;
  text-transform: uppercase;
  font-size: 10px;
}

.level-debug { color: #9ca3af; }
.level-info { color: #60a5fa; }
.level-warning { color: #fbbf24; }
.level-error { color: #f87171; }

.log-action {
  color: var(--color-text);
  font-weight: 500;
  flex-shrink: 0;
}

.log-status {
  color: var(--color-text-muted);
  flex-shrink: 0;
}

.log-detail {
  color: var(--color-text-muted);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
