<script setup lang="ts">
import { computed, ref, useId } from 'vue';
import BaseDialog from '../dialogs/BaseDialog.vue';
import type { Profile } from '../../types';
import { intlLocale, t } from '../../i18n';

const props = defineProps<{
  mode: 'import' | 'export';
  /// Export: the profiles that can be exported. Import: those read from the file.
  profiles: Profile[];
  /// Export: only these groups ticked at first ('' for ungrouped). All when absent.
  onlyGroup?: string;
  /// Import: profiles already in the app, to spot ones that are already there.
  existing?: Profile[];
  /// Import: groups already in use, offered as targets.
  groups?: string[];
}>();

const emit = defineEmits<{ confirm: [profiles: Profile[]]; cancel: [] }>();

const collator = new Intl.Collator(intlLocale(), { numeric: true, sensitivity: 'base' });
const ids = useId();

function sameServer(a: Profile, b: Profile) {
  return (
    a.protocol === b.protocol &&
    a.host.toLowerCase() === b.host.toLowerCase() &&
    a.port === b.port &&
    a.username === b.username
  );
}

const duplicates = computed(
  () => new Set(props.profiles.filter((p) => props.existing?.some((e) => sameServer(e, p))).map((p) => p.id)),
);

const checked = ref(
  new Set(
    props.profiles
      .filter((p) =>
        props.mode === 'import'
          ? !duplicates.value.has(p.id)
          : props.onlyGroup === undefined || (p.group ?? '') === props.onlyGroup,
      )
      .map((p) => p.id),
  ),
);

/// Import: when set, every imported profile goes into this group.
const targetGroup = ref('');

const sections = computed(() => {
  const byGroup = new Map<string, Profile[]>();
  for (const p of props.profiles) {
    const key = p.group ?? '';
    byGroup.set(key, [...(byGroup.get(key) ?? []), p]);
  }
  return [...byGroup.entries()]
    .sort(([a], [b]) => (a === '' ? -1 : b === '' ? 1 : collator.compare(a, b)))
    .map(([name, items]) => ({ name, items }));
});

function toggle(id: string) {
  const next = new Set(checked.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  checked.value = next;
}

function sectionState(items: Profile[]) {
  const n = items.filter((p) => checked.value.has(p.id)).length;
  return n === 0 ? 'none' : n === items.length ? 'all' : 'some';
}

function toggleSection(items: Profile[]) {
  const next = new Set(checked.value);
  const add = sectionState(items) !== 'all';
  for (const p of items) {
    if (add) next.add(p.id);
    else next.delete(p.id);
  }
  checked.value = next;
}

function setAll(on: boolean) {
  checked.value = new Set(on ? props.profiles.map((p) => p.id) : []);
}

const selection = computed(() => props.profiles.filter((p) => checked.value.has(p.id)));
const keyPaths = computed(() => selection.value.some((p) => 'path' in p.auth_method));

function onConfirm() {
  const group = targetGroup.value.trim();
  emit(
    'confirm',
    props.mode === 'import' && group ? selection.value.map((p) => ({ ...p, group })) : selection.value,
  );
}
</script>

<template>
  <BaseDialog :title="mode === 'import' ? t.exchange.importTitle : t.exchange.exportTitle" :width="480" @cancel="emit('cancel')">
    <div class="toolbar">
      <span role="status">{{ t.exchange.selected(selection.length, profiles.length) }}</span>
      <span class="links">
        <button class="link-btn" @click="setAll(true)">{{ t.exchange.all }}</button>
        <button class="link-btn" @click="setAll(false)">{{ t.exchange.none }}</button>
      </span>
    </div>

    <div class="list" role="group" :aria-label="t.exchange.list">
      <section v-for="section in sections" :key="section.name">
        <label v-if="sections.length > 1 || section.name" class="section">
          <input
            type="checkbox"
            :checked="sectionState(section.items) === 'all'"
            :indeterminate="sectionState(section.items) === 'some'"
            @change="toggleSection(section.items)"
          />
          {{ section.name || t.exchange.ungrouped }}
        </label>
        <label v-for="p in section.items" :key="p.id" class="item" :class="{ nested: sections.length > 1 || section.name }">
          <input type="checkbox" :checked="checked.has(p.id)" @change="toggle(p.id)" />
          <span class="name">{{ p.name }}</span>
          <span class="detail">{{ p.protocol }} · {{ p.username }}@{{ p.host }}:{{ p.port }}</span>
          <span v-if="duplicates.has(p.id)" class="badge" :title="t.exchange.duplicateHint">{{ t.exchange.duplicate }}</span>
        </label>
      </section>
    </div>

    <label v-if="mode === 'import'" class="field">
      <span class="form-label">{{ t.exchange.targetGroup }}</span>
      <input
        v-model="targetGroup"
        class="form-input"
        :list="`${ids}-groups`"
        :placeholder="t.exchange.keepGroups"
        spellcheck="false"
      />
      <datalist :id="`${ids}-groups`">
        <option v-for="g in groups ?? []" :key="g" :value="g" />
      </datalist>
    </label>

    <p class="note">
      <template v-if="mode === 'export'">
        {{ t.exchange.noSecrets }}
        <template v-if="keyPaths">{{ t.exchange.keyPaths }}</template>
      </template>
      <template v-else>{{ t.exchange.importNote }}</template>
    </p>

    <template #footer>
      <button class="btn" @click="emit('cancel')">{{ t.common.cancel }}</button>
      <button class="btn btn-primary" :disabled="selection.length === 0" @click="onConfirm">
        {{ mode === 'import' ? t.exchange.import(selection.length) : t.exchange.export(selection.length) }}
      </button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.toolbar {
  display: flex;
  justify-content: space-between;
  color: var(--color-text-muted);
}

.links {
  display: flex;
  gap: 10px;
}

.link-btn {
  padding: 0;
  border: none;
  background: none;
  color: var(--color-accent);
  font: inherit;
  cursor: pointer;
  text-decoration: underline;
}

.list {
  max-height: 300px;
  overflow-y: auto;
  padding: 4px 0;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
}

.section,
.item {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 8px;
  cursor: pointer;
}

.section {
  font-weight: 600;
  color: var(--color-text-muted);
}

.item.nested {
  padding-left: 26px;
}

.item:hover,
.section:hover {
  background: var(--color-hover);
}

.name {
  flex-shrink: 0;
  max-width: 45%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.detail {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--color-text-muted);
  font-size: 11px;
}

.badge {
  flex-shrink: 0;
  padding: 0 5px;
  border-radius: 3px;
  background: var(--bg-warning);
  color: var(--color-warning);
  font-size: 10px;
}

.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.note {
  margin: 0;
  color: var(--color-text-muted);
  font-size: 11px;
}
</style>
