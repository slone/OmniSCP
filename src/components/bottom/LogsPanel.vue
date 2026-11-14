<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from 'vue';
import { useLogStore } from '../../stores/logs';
import { errorMessage } from '../../utils/errors';
import { intlLocale, t } from '../../i18n';

const logStore = useLogStore();

const list = ref<HTMLElement | null>(null);
const confirmingClear = ref(false);
const message = ref('');

function scrollToEnd() {
  nextTick(() => {
    if (list.value) list.value.scrollTop = list.value.scrollHeight;
  });
}

onMounted(scrollToEnd);
watch(() => logStore.entries.length, scrollToEnd);

// The journal keeps earlier sessions, so the time alone would be ambiguous.
function formatTime(timestamp: number): string {
  return new Date(timestamp * 1000).toLocaleString(intlLocale(), {
    day: '2-digit',
    month: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  });
}

async function onExport() {
  try {
    message.value = t.logs.exported(await logStore.exportToDownloads());
  } catch (err) {
    message.value = t.logs.exportFailed(errorMessage(err));
  }
}

async function onClear() {
  confirmingClear.value = false;
  try {
    await logStore.clear();
    message.value = '';
  } catch (err) {
    message.value = t.logs.clearFailed(errorMessage(err));
  }
}
</script>

<template>
  <div class="logs-panel">
    <div class="logs-toolbar">
      <span class="logs-message" :title="message" role="status">
        <template v-if="logStore.saveError">{{ t.logs.notSaved(logStore.saveError) }}</template>
        <template v-else>{{ message }}</template>
      </span>
      <button class="logs-btn" :disabled="logStore.entries.length === 0" @click="onExport">{{ t.logs.export }}</button>
      <template v-if="confirmingClear">
        <button class="logs-btn logs-btn-danger" @click="onClear">{{ t.logs.confirmClear }}</button>
        <button class="logs-btn" @click="confirmingClear = false">{{ t.common.cancel }}</button>
      </template>
      <button v-else class="logs-btn" :disabled="logStore.entries.length === 0" @click="confirmingClear = true">
        {{ t.logs.clear }}
      </button>
    </div>
    <div ref="list" class="log-entries" role="log" aria-live="off" :aria-label="t.logs.label" tabindex="0">
      <div
        v-for="(entry, i) in logStore.entries"
        :key="i"
        :class="['log-entry', 'log-' + entry.level.toLowerCase()]"
      >
        <span class="log-time">{{ formatTime(entry.timestamp) }}</span>
        <span :class="['log-level', 'level-' + entry.level.toLowerCase()]">
          {{ t.logs.levels[entry.level] ?? entry.level }}
        </span>
        <span class="log-action">{{ entry.action }}</span>
        <span class="log-status">{{ entry.status }}</span>
        <span v-if="entry.detail" class="log-detail" :title="entry.detail">{{ entry.detail }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.logs-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  font-family: var(--font-mono);
  font-size: 11px;
}

.logs-toolbar {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 2px 8px;
  border-bottom: 1px solid var(--color-border);
  font-family: var(--font-sans);
  color: var(--color-text-muted);
}

.logs-message {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.logs-btn {
  padding: 1px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: none;
  color: var(--color-text);
  font-size: 11px;
  cursor: pointer;
}

.logs-btn:hover:not(:disabled) {
  background: var(--color-hover);
}

.logs-btn:disabled {
  opacity: 0.4;
  cursor: default;
}

.logs-btn-danger {
  border-color: var(--color-danger-solid);
  color: var(--color-danger);
}

.log-entries {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
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

.level-debug { color: var(--color-neutral); }
.level-info { color: var(--color-info); }
.level-warning { color: var(--color-warning); }
.level-error { color: var(--color-danger); }

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
