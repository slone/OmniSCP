<script setup lang="ts">
import { ref } from 'vue';
import TransferQueue from './TransferQueue.vue';
import LogsPanel from './LogsPanel.vue';

const activeTab = ref<'queue' | 'logs'>('queue');
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
      <div class="tab-spacer" />
    </div>

    <div class="panel-content">
      <TransferQueue v-if="activeTab === 'queue'" />
      <LogsPanel v-if="activeTab === 'logs'" />
    </div>
  </div>
</template>

<style scoped>
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
