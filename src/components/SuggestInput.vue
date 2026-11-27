<script setup lang="ts">
// A text field offering existing values, as an editable combobox with a list
// (WAI-ARIA 1.2). It replaces <datalist>, whose list WebKit draws outside the
// page and leaves on screen when the field goes away.

import { computed, ref, useId } from 'vue';
import { t } from '../i18n';

const props = defineProps<{
  modelValue: string;
  suggestions: string[];
  id?: string;
  placeholder?: string;
}>();

const emit = defineEmits<{ 'update:modelValue': [value: string] }>();

const listId = useId();
const open = ref(false);
/// The highlighted suggestion, -1 for none.
const active = ref(-1);
/// Opened with the button or a key: every suggestion, whatever is typed.
const showAll = ref(false);

const shown = computed(() => {
  const needle = props.modelValue.trim().toLowerCase();
  const all = props.suggestions;
  if (showAll.value || !needle) return all;
  return all.filter((s) => s.toLowerCase().includes(needle) && s !== props.modelValue);
});

const expanded = computed(() => open.value && shown.value.length > 0);
const optionId = (i: number) => `${listId}-${i}`;

function openList(all: boolean) {
  showAll.value = all;
  open.value = true;
  active.value = -1;
}

function close() {
  open.value = false;
  active.value = -1;
}

const input = ref<HTMLInputElement | null>(null);

/// The ▾ button: the field takes the focus, so the list closes with it.
function toggle() {
  input.value?.focus();
  if (expanded.value) close();
  else openList(true);
}

function pick(value: string) {
  emit('update:modelValue', value);
  close();
}

function onInput(e: Event) {
  emit('update:modelValue', (e.target as HTMLInputElement).value);
  openList(false);
}

function onKeydown(e: KeyboardEvent) {
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    if (!expanded.value) {
      openList(true);
      if (shown.value.length > 0) active.value = e.key === 'ArrowDown' ? 0 : shown.value.length - 1;
      return;
    }
    const step = e.key === 'ArrowDown' ? 1 : -1;
    active.value = (active.value + step + shown.value.length) % shown.value.length;
  } else if (e.key === 'Enter' && expanded.value && active.value >= 0) {
    // Picks the suggestion rather than submitting the form.
    e.preventDefault();
    pick(shown.value[active.value]);
  } else if (e.key === 'Escape' && expanded.value) {
    // Closes the list, not the dialog.
    e.preventDefault();
    e.stopPropagation();
    close();
  }
}
</script>

<template>
  <div class="suggest">
    <input
      :id="id"
      ref="input"
      :value="modelValue"
      type="text"
      class="suggest-input"
      role="combobox"
      aria-autocomplete="list"
      :aria-expanded="expanded"
      :aria-controls="listId"
      :aria-activedescendant="expanded && active >= 0 ? optionId(active) : undefined"
      :placeholder="placeholder"
      autocomplete="off"
      spellcheck="false"
      @input="onInput"
      @keydown="onKeydown"
      @blur="close"
    />
    <button
      v-if="suggestions.length > 0"
      type="button"
      class="suggest-toggle"
      tabindex="-1"
      :aria-label="t.common.showSuggestions"
      @mousedown.prevent
      @click="toggle"
    >
      ▾
    </button>
    <ul v-show="expanded" :id="listId" class="suggest-list" role="listbox">
      <li
        v-for="(s, i) in shown"
        :id="optionId(i)"
        :key="s"
        role="option"
        :aria-selected="i === active"
        :class="{ active: i === active }"
        @mousedown.prevent="pick(s)"
      >
        {{ s }}
      </li>
    </ul>
  </div>
</template>

<style scoped>
.suggest {
  position: relative;
  display: flex;
}

/* As the dialogs' other fields (BaseDialog's .form-input). */
.suggest-input {
  flex: 1;
  min-width: 0;
  padding: 6px 26px 6px 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  color: var(--color-text);
  font-size: 13px;
  font-family: inherit;
  outline: none;
}

.suggest-input:focus {
  border-color: var(--color-accent);
}

.suggest-input:focus-visible {
  outline: 2px solid var(--color-accent);
  outline-offset: 1px;
}

.suggest-toggle {
  position: absolute;
  top: 1px;
  right: 1px;
  bottom: 1px;
  width: 24px;
  border: none;
  background: none;
  color: var(--color-text-muted);
  font-size: 11px;
  cursor: pointer;
}

.suggest-list {
  position: absolute;
  top: calc(100% + 2px);
  left: 0;
  right: 0;
  z-index: 10;
  max-height: 160px;
  margin: 0;
  padding: 2px 0;
  overflow-y: auto;
  list-style: none;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-surface);
  box-shadow: 0 4px 12px rgb(0 0 0 / 25%);
  font-size: 13px;
}

.suggest-list li {
  padding: 4px 8px;
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.suggest-list li:hover,
.suggest-list li.active {
  background: var(--color-selected, var(--color-active));
  color: var(--color-text);
}
</style>
