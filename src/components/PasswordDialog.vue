<script setup lang="ts">
import { ref, onMounted } from 'vue';

const props = defineProps<{
  profileName: string;
  promptLabel: string;
  notice?: string;
  rememberDefault?: boolean;
}>();

const emit = defineEmits<{
  submit: [password: string, remember: boolean];
  cancel: [];
}>();

const password = ref('');
const remember = ref(props.rememberDefault ?? false);
const inputRef = ref<HTMLInputElement | null>(null);

onMounted(() => {
  inputRef.value?.focus();
});

function onSubmit() {
  emit('submit', password.value, remember.value);
}

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
        <span class="dialog-title">Authentification — {{ profileName }}</span>
      </div>

      <form class="dialog-body" @submit.prevent="onSubmit">
        <p v-if="notice" class="notice">{{ notice }}</p>
        <div class="form-group">
          <label class="form-label">{{ promptLabel }}</label>
          <input
            ref="inputRef"
            v-model="password"
            type="password"
            class="form-input"
            autocomplete="off"
          />
        </div>
        <label class="checkbox-label">
          <input v-model="remember" type="checkbox" />
          Enregistrer dans le trousseau du système
        </label>
      </form>

      <div class="dialog-footer">
        <button class="btn btn-cancel" @click="emit('cancel')">Annuler</button>
        <button class="btn btn-save" @click="onSubmit">Connexion</button>
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
  width: 380px;
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

.notice {
  font-size: 12px;
  color: #fbbf24;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: var(--color-text);
  cursor: pointer;
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
