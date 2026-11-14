<script setup lang="ts">
import { computed, ref, useId, watch } from 'vue';
import TransferQueue from './TransferQueue.vue';
import LogsPanel from './LogsPanel.vue';
import EditsPanel from './EditsPanel.vue';
import { useEditsStore } from '../../stores/edits';
import { t } from '../../i18n';

const edits = useEditsStore();
type Tab = 'queue' | 'logs' | 'edits';
const activeTab = ref<Tab>('queue');
const ids = useId();

const tabs = computed(() => {
  const list: { key: Tab; label: string; alert?: boolean }[] = [
    { key: 'queue', label: t.bottom.queue },
    { key: 'logs', label: t.bottom.logs },
  ];
  if (edits.files.length > 0) {
    list.push({
      key: 'edits',
      label: t.bottom.edits(edits.files.length),
      alert: edits.files.some((f) => f.state === 'conflict' || f.state === 'error'),
    });
  }
  return list;
});

/// Arrow keys move between tabs, as in other tab lists.
function onTabKeydown(e: KeyboardEvent) {
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
  const keys = tabs.value.map((tab) => tab.key);
  const next = (keys.indexOf(activeTab.value) + (e.key === 'ArrowRight' ? 1 : -1) + keys.length) % keys.length;
  activeTab.value = keys[next];
  document.getElementById(`${ids}-tab-${keys[next]}`)?.focus();
}

// A refused save must not go unnoticed.
watch(() => edits.attention, () => (activeTab.value = 'edits'));
watch(
  () => edits.files.length,
  (count, before) => {
    if (count > before) activeTab.value = 'edits';
    else if (count === 0 && activeTab.value === 'edits') activeTab.value = 'queue';
  },
);
</script>

<template>
  <div class="bottom-panel">
    <div class="panel-tabs" role="tablist" :aria-label="t.bottom.label" @keydown="onTabKeydown">
      <button
        v-for="tab in tabs"
        :id="`${ids}-tab-${tab.key}`"
        :key="tab.key"
        role="tab"
        :aria-selected="activeTab === tab.key"
        :aria-controls="`${ids}-panel`"
        :tabindex="activeTab === tab.key ? 0 : -1"
        :class="['tab-btn', { active: activeTab === tab.key }]"
        @click="activeTab = tab.key"
      >
        {{ tab.label }}
        <span v-if="tab.alert" class="tab-alert" role="img" :aria-label="t.bottom.needsAttention">!</span>
      </button>
      <div class="tab-spacer" />
    </div>

    <div :id="`${ids}-panel`" class="panel-content" role="tabpanel" :aria-labelledby="`${ids}-tab-${activeTab}`">
      <TransferQueue v-if="activeTab === 'queue'" />
      <LogsPanel v-if="activeTab === 'logs'" />
      <EditsPanel v-if="activeTab === 'edits'" />
    </div>
  </div>
</template>

<style scoped>
.tab-alert {
  margin-left: 4px;
  padding: 0 5px;
  border-radius: 8px;
  background: var(--color-danger-solid);
  color: var(--color-on-accent);
  font-size: 10px;
}

.bottom-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  background: var(--color-bg);
  border-top: 1px solid var(--color-border);
}

.panel-tabs {
  display: flex;
  align-items: center;
  background: var(--color-surface);
  border-bottom: 1px solid var(--color-border);
  min-height: 28px;
}

.tab-btn {
  padding: 4px 12px;
  border: none;
  border-bottom: 2px solid transparent;
  background: none;
  color: var(--color-text-muted);
  font-size: 12px;
  cursor: pointer;
}

.tab-btn:hover {
  color: var(--color-text);
  background: var(--color-hover);
}

.tab-btn.active {
  color: var(--color-text);
  border-bottom-color: var(--color-accent);
}

.tab-spacer {
  flex: 1;
}

.panel-content {
  flex: 1;
  overflow: hidden;
}
</style>
