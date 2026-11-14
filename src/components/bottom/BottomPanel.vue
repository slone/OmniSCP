<script setup lang="ts">
import { ref, watch } from 'vue';
import TransferQueue from './TransferQueue.vue';
import LogsPanel from './LogsPanel.vue';
import EditsPanel from './EditsPanel.vue';
import { useEditsStore } from '../../stores/edits';

const edits = useEditsStore();
const activeTab = ref<'queue' | 'logs' | 'edits'>('queue');

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
    <div class="panel-tabs">
      <button
        :class="['tab-btn', { active: activeTab === 'queue' }]"
        @click="activeTab = 'queue'"
      >
        File d'attente
      </button>
      <button
        :class="['tab-btn', { active: activeTab === 'logs' }]"
        @click="activeTab = 'logs'"
      >
        Journal
      </button>
      <button
        v-if="edits.files.length > 0"
        :class="['tab-btn', { active: activeTab === 'edits' }]"
        @click="activeTab = 'edits'"
      >
        En édition ({{ edits.files.length }})
        <span v-if="edits.files.some((f) => f.state === 'conflict' || f.state === 'error')" class="tab-alert" aria-label="à vérifier">!</span>
      </button>
      <div class="tab-spacer" />
    </div>

    <div class="panel-content">
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
