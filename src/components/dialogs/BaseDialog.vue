<script setup lang="ts">
withDefaults(defineProps<{ title: string; width?: number }>(), { width: 380 });

const emit = defineEmits<{ cancel: [] }>();

function onOverlayClick(e: MouseEvent) {
  if ((e.target as HTMLElement).classList.contains('dialog-overlay')) {
    emit('cancel');
  }
}
</script>

<template>
  <div class="dialog-overlay" @click="onOverlayClick" @keydown.esc="emit('cancel')">
    <div class="dialog-panel" :style="{ width: width + 'px' }">
      <div class="dialog-header">
        <span class="dialog-title">{{ title }}</span>
      </div>
      <div class="dialog-body">
        <slot />
      </div>
      <div class="dialog-footer">
        <slot name="footer" />
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
  background: var(--color-overlay);
}

.dialog-panel {
  max-width: calc(100vw - 32px);
  display: flex;
  flex-direction: column;
  background: var(--color-surface);
  border: 1px solid var(--color-border);
  border-radius: 6px;
  box-shadow: 0 8px 32px var(--shadow-color);
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
  font-size: 12px;
  color: var(--color-text);
}

.dialog-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--color-border);
}

/* Shared by dialogs built on this component */
:slotted(.btn) {
  padding: 6px 16px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  font-size: 12px;
  cursor: pointer;
  font-family: inherit;
  background: none;
  color: var(--color-text);
}

:slotted(.btn:hover) {
  background: var(--color-hover);
}

:slotted(.btn-primary) {
  background: var(--color-accent);
  border-color: var(--color-accent);
  color: var(--color-on-accent);
}

:slotted(.btn-primary:hover) {
  background: var(--color-accent-hover);
}

:slotted(.btn-danger) {
  background: var(--color-danger-solid);
  border-color: var(--color-danger-solid);
  color: var(--color-on-accent);
}

:slotted(.btn-danger:hover) {
  background: var(--color-danger-solid-hover);
}

:slotted(.form-input) {
  padding: 6px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}

:slotted(.form-input:focus) {
  border-color: var(--color-accent);
}

:slotted(.form-label) {
  font-size: 11px;
  font-weight: 500;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}
</style>
