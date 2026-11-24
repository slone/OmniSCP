<script setup lang="ts">
// A vertical bar that resizes what is on either side of it, with the mouse
// or, once focused, with the keyboard (the WAI-ARIA window splitter): arrows
// step, Home and End go to the limits, Enter or a double-click resets.

const props = withDefaults(
  defineProps<{
    modelValue: number;
    min: number;
    max: number;
    /// One arrow key press.
    step: number;
    /// Units per pixel of mouse movement: 1 for pixels, 1/width for a fraction.
    scale?: () => number;
    /// Where Enter and a double-click bring it back.
    reset: number;
    label: string;
    /// How a screen reader reads the value.
    valueText?: (value: number) => string;
  }>(),
  { scale: () => 1, valueText: undefined },
);

const emit = defineEmits<{
  'update:modelValue': [value: number];
  /// The value settled: a drag ended or a key was pressed.
  commit: [];
}>();

function set(value: number) {
  emit('update:modelValue', Math.min(props.max, Math.max(props.min, value)));
}

function onMouseDown(e: MouseEvent) {
  if (e.button !== 0) return;
  e.preventDefault();
  (e.currentTarget as HTMLElement).focus();
  const startX = e.clientX;
  const start = props.modelValue;
  const scale = props.scale();
  const move = (m: MouseEvent) => set(start + (m.clientX - startX) * scale);
  const up = () => {
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    document.documentElement.classList.remove('col-resizing');
    emit('commit');
  };
  document.documentElement.classList.add('col-resizing');
  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
}

function onKeyDown(e: KeyboardEvent) {
  const moves: Record<string, () => number> = {
    ArrowLeft: () => props.modelValue - props.step,
    ArrowRight: () => props.modelValue + props.step,
    Home: () => props.min,
    End: () => props.max,
    Enter: () => props.reset,
  };
  const next = moves[e.key];
  if (!next) return;
  e.preventDefault();
  set(next());
  emit('commit');
}

function onDblClick() {
  set(props.reset);
  emit('commit');
}
</script>

<template>
  <div
    class="splitter"
    role="separator"
    tabindex="0"
    aria-orientation="vertical"
    :aria-label="label"
    :aria-valuenow="Math.round(modelValue)"
    :aria-valuemin="Math.round(min)"
    :aria-valuemax="Math.round(max)"
    :aria-valuetext="valueText?.(modelValue)"
    @mousedown="onMouseDown"
    @keydown="onKeyDown"
    @dblclick="onDblClick"
  />
</template>

<style scoped>
/* Wider than it looks, so it is easy to catch: a 1px line in a 7px band. */
.splitter {
  width: 7px;
  flex-shrink: 0;
  cursor: col-resize;
  background: linear-gradient(to right, transparent 3px, var(--color-border) 3px, var(--color-border) 4px, transparent 4px);
}

.splitter:hover,
.splitter:focus-visible {
  outline: none;
  background: linear-gradient(to right, transparent 2px, var(--color-accent) 2px, var(--color-accent) 5px, transparent 5px);
}
</style>
