<script setup lang="ts">
import { ref } from 'vue';
import BaseDialog from './dialogs/BaseDialog.vue';
import { useSettingsStore } from '../stores/settings';
import { useConnectionStore } from '../stores/connection';
import type { AppConfig } from '../types';
import { t } from '../i18n';

const MIN_TRANSFERS = 1;
const MAX_TRANSFERS = 4;

const emit = defineEmits<{ close: [] }>();

const store = useSettingsStore();
const connection = useConnectionStore();
const draft = ref<AppConfig>({ ...store.settings });
const saving = ref(false);
const failed = ref(false);

async function onSave() {
  saving.value = true;
  failed.value = false;
  draft.value.concurrent_transfers = Math.min(
    MAX_TRANSFERS,
    Math.max(MIN_TRANSFERS, Math.round(draft.value.concurrent_transfers) || MIN_TRANSFERS),
  );
  const ok = await store.save(draft.value);
  saving.value = false;
  if (ok) emit('close');
  else failed.value = true;
}
</script>

<template>
  <BaseDialog :title="t.settings.title" :width="440" @cancel="emit('close')">
    <form class="settings" @submit.prevent="onSave">
      <label class="field">
        <span class="form-label">{{ t.settings.language }}</span>
        <select v-model="draft.language" class="form-input">
          <option value="system">{{ t.settings.languageSystem }}</option>
          <option value="fr" lang="fr">Français</option>
          <option value="en" lang="en">English</option>
        </select>
      </label>

      <label class="field">
        <span class="form-label">{{ t.settings.theme }}</span>
        <select v-model="draft.theme" class="form-input">
          <option value="system">{{ t.settings.themeSystem }}</option>
          <option value="dark">{{ t.settings.themeDark }}</option>
          <option value="light">{{ t.settings.themeLight }}</option>
        </select>
      </label>

      <label class="field">
        <span class="form-label">{{ t.settings.transfers }}</span>
        <input
          v-model.number="draft.concurrent_transfers"
          type="number"
          class="form-input narrow"
          :min="MIN_TRANSFERS"
          :max="MAX_TRANSFERS"
        />
        <span class="hint">
          {{ t.settings.transfersHint(MIN_TRANSFERS, MAX_TRANSFERS) }}
          <template v-if="connection.connected">{{ t.settings.nextConnection }}</template>
        </span>
      </label>

      <label class="field">
        <span class="form-label">{{ t.settings.conflict }}</span>
        <select v-model="draft.default_conflict" class="form-input">
          <option value="Ask">{{ t.settings.conflictAsk }}</option>
          <option value="Skip">{{ t.settings.conflictSkip }}</option>
          <option value="Rename">{{ t.settings.conflictRename }}</option>
          <option value="Overwrite">{{ t.settings.conflictOverwrite }}</option>
        </select>
        <span class="hint">{{ t.settings.conflictHint }}</span>
      </label>

      <label class="check">
        <input v-model="draft.show_hidden_files" type="checkbox" />
        {{ t.settings.hidden }}
      </label>

      <p v-if="failed" class="error" role="alert">
        {{ t.settings.saveFailed }}
      </p>
    </form>

    <template #footer>
      <button class="btn" @click="emit('close')">{{ t.common.cancel }}</button>
      <button class="btn btn-primary" :disabled="saving" @click="onSave">{{ t.common.save }}</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.settings {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.narrow {
  width: 80px;
}

.hint {
  color: var(--color-text-muted);
  font-size: 11px;
}

.check {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}

.error {
  color: var(--color-danger);
}
</style>
