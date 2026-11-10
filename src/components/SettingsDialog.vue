<script setup lang="ts">
import { ref } from 'vue';
import BaseDialog from './dialogs/BaseDialog.vue';
import { useSettingsStore } from '../stores/settings';
import { useConnectionStore } from '../stores/connection';
import type { AppConfig } from '../types';

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
  <BaseDialog title="Paramètres" :width="440" @cancel="emit('close')">
    <form class="settings" @submit.prevent="onSave">
      <label class="field">
        <span class="form-label">Thème</span>
        <select v-model="draft.theme" class="form-input">
          <option value="system">Comme le système</option>
          <option value="dark">Sombre</option>
          <option value="light">Clair</option>
        </select>
      </label>

      <label class="field">
        <span class="form-label">Transferts simultanés</span>
        <input
          v-model.number="draft.concurrent_transfers"
          type="number"
          class="form-input narrow"
          :min="MIN_TRANSFERS"
          :max="MAX_TRANSFERS"
        />
        <span class="hint">
          De {{ MIN_TRANSFERS }} à {{ MAX_TRANSFERS }}. Chaque transfert ouvre sa propre session sur le serveur.
          <template v-if="connection.connected">Prend effet à la prochaine connexion.</template>
        </span>
      </label>

      <label class="field">
        <span class="form-label">Si un fichier existe déjà à la destination</span>
        <select v-model="draft.default_conflict" class="form-input">
          <option value="Ask">Demander à chaque fois</option>
          <option value="Skip">Ignorer le fichier</option>
          <option value="Rename">Copier sous un autre nom</option>
          <option value="Overwrite">Écraser sans demander</option>
        </select>
        <span class="hint">Pour les envois et téléchargements. La synchronisation a ses propres choix.</span>
      </label>

      <label class="check">
        <input v-model="draft.show_hidden_files" type="checkbox" />
        Afficher les fichiers cachés (nom commençant par un point)
      </label>

      <p v-if="failed" class="error" role="alert">
        Les paramètres n'ont pas pu être enregistrés. Le détail est dans le journal.
      </p>
    </form>

    <template #footer>
      <button class="btn" @click="emit('close')">Annuler</button>
      <button class="btn btn-primary" :disabled="saving" @click="onSave">Enregistrer</button>
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
