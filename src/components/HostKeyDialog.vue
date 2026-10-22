<script setup lang="ts">
import { ref, onMounted } from 'vue';

defineProps<{
  profileName: string;
  host: string;
  port: number;
  fingerprint: string;
}>();

const emit = defineEmits<{
  accept: [];
  cancel: [];
}>();

const cancelRef = ref<HTMLButtonElement | null>(null);

onMounted(() => {
  // Focus the safe choice so Enter does not trust the key by accident.
  cancelRef.value?.focus();
});

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    emit('cancel');
  }
}

function onOverlayClick(e: MouseEvent) {
  if ((e.target as HTMLElement).classList.contains('dialog-overlay')) {
    emit('cancel');
  }
}
</script>

<template>
  <div class="dialog-overlay" @click="onOverlayClick" @keydown="onKeydown">
    <div class="dialog-panel">
      <div class="dialog-header">
        <span class="dialog-title">Serveur inconnu — {{ profileName }}</span>
      </div>

      <div class="dialog-body">
        <p class="dialog-text">
          L'identité du serveur <strong>{{ host }}:{{ port }}</strong> n'a encore jamais été vérifiée.
          Comparez cette empreinte avec celle fournie par l'administrateur du serveur avant de continuer.
        </p>
        <div class="fingerprint">{{ fingerprint }}</div>
        <p class="dialog-text">Si vous l'acceptez, elle sera mémorisée pour les prochaines connexions.</p>
      </div>

      <div class="dialog-footer">
        <button ref="cancelRef" class="btn btn-cancel" @click="emit('cancel')">Annuler</button>
        <button class="btn btn-save" @click="emit('accept')">Faire confiance et se connecter</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialog-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
}

.dialog-panel {
  width: 440px;
  display: flex;
  flex-direction: column;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
}

.dialog-header {
  padding: 12px 16px;
  border-bottom: 1px solid var(--color-border);
}

.dialog-title {
  font-size: 14px;
  font-weight: 600;
  color: var(--color-text);
}

.dialog-body {
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.dialog-text {
  font-size: 12px;
  color: var(--color-text);
}

.fingerprint {
  padding: 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  font-family: var(--font-mono);
  font-size: 12px;
  word-break: break-all;
  user-select: all;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.form-label {
  font-size: 11px;
  font-weight: 500;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}

.form-input {
  padding: 6px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}

.form-input:focus {
  border-color: var(--color-accent);
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--color-border);
}

.btn {
  padding: 6px 16px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
}

.btn-cancel {
  background: none;
  color: var(--color-text-muted);
}

.btn-cancel:hover {
  background: var(--color-hover);
  color: var(--color-text);
}

.btn-save {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: #fff;
}

.btn-save:hover {
  background: #006cbd;
}
</style>
