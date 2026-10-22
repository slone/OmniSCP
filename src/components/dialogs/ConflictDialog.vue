<script setup lang="ts">
import { ref } from 'vue';
import BaseDialog from './BaseDialog.vue';
import type { ConflictChoice } from '../../stores/fileActions';

defineProps<{ name: string; remaining: number }>();
const emit = defineEmits<{ answer: [choice: ConflictChoice, applyToAll: boolean] }>();

const applyToAll = ref(false);
</script>

<template>
  <BaseDialog title="Fichier existant" :width="420" @cancel="emit('answer', 'Cancel', false)">
    <p>
      <strong>{{ name }}</strong> existe déjà dans le dossier de destination.
    </p>
    <label v-if="remaining > 0" class="apply-all">
      <input v-model="applyToAll" type="checkbox" />
      Appliquer aux {{ remaining }} autre(s) conflit(s)
    </label>
    <template #footer>
      <button class="btn" @click="emit('answer', 'Cancel', false)">Annuler</button>
      <button class="btn" @click="emit('answer', 'Skip', applyToAll)">Ignorer</button>
      <button class="btn" @click="emit('answer', 'Rename', applyToAll)">Renommer</button>
      <button class="btn btn-primary" @click="emit('answer', 'Overwrite', applyToAll)">Écraser</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.apply-all {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}
</style>
