<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, useId } from 'vue';

withDefaults(defineProps<{ title: string; width?: number }>(), { width: 380 });

const emit = defineEmits<{ cancel: [] }>();

const titleId = useId();
const panel = ref<HTMLElement | null>(null);
/// Gets the focus back when the dialog closes.
const opener = document.activeElement as HTMLElement | null;

const FOCUSABLE = 'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])';

function focusable(): HTMLElement[] {
  return [...(panel.value?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])].filter(
    (el) => !el.hasAttribute('disabled') && el.offsetParent !== null,
  );
}

onMounted(async () => {
  // Dialogs focus their own safe or main control; otherwise the first one.
  await nextTick();
  if (!panel.value?.contains(document.activeElement)) focusable()[0]?.focus();
});

onBeforeUnmount(() => {
  // WebKit leaves a field's suggestion list on screen when the field goes
  // away while focused (a group picked, then saved at once).
  const active = document.activeElement;
  if (active instanceof HTMLElement && panel.value?.contains(active)) active.blur();
  if (opener?.isConnected) opener.focus();
});

/// Keeps Tab inside the dialog while it is open.
function onTab(e: KeyboardEvent) {
  const items = focusable();
  if (items.length === 0) return;
  const first = items[0];
  const last = items[items.length - 1];
  if (e.shiftKey && document.activeElement === first) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}

function onOverlayClick(e: MouseEvent) {
  if ((e.target as HTMLElement).classList.contains('dialog-overlay')) {
    emit('cancel');
  }
}
</script>

<template>
  <div class="dialog-overlay" @click="onOverlayClick" @keydown.esc="emit('cancel')" @keydown.tab="onTab">
    <div
      ref="panel"
      class="dialog-panel"
      role="dialog"
      aria-modal="true"
      :aria-labelledby="titleId"
      :style="{ width: width + 'px' }"
    >
      <div class="dialog-header">
        <h2 :id="titleId" class="dialog-title">{{ title }}</h2>
      </div>
      <div class="dialog-body">
        <slot />
      </div>
      <div v-if="$slots.status" class="dialog-status">
        <slot name="status" />
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
  max-height: calc(100vh - 32px);
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
  margin: 0;
  font-size: 14px;
  font-weight: 600;
  color: var(--color-text);
}

.dialog-body {
  overflow-y: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  font-size: 12px;
  color: var(--color-text);
}

.dialog-status {
  padding: 0 16px 12px;
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

:slotted(.btn:focus-visible),
:slotted(.form-input:focus-visible) {
  outline: 2px solid var(--color-accent);
  outline-offset: 1px;
}

:slotted(.form-label) {
  font-size: 11px;
  font-weight: 500;
  color: var(--color-text-muted);
  text-transform: uppercase;
  letter-spacing: 0.3px;
}
</style>
