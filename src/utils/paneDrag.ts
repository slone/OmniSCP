// Dragging files between the panes, and dropping files from the system's
// file manager.
//
// Between the panes this follows the mouse instead of using HTML drag and
// drop, which the webview handles differently on each system and which on
// Windows cannot work alongside drops from outside.

import { reactive } from 'vue';
import type { PaneSide } from '../types';

export interface PaneDrag {
  from: PaneSide;
  paths: string[];
  /// What the label next to the pointer shows: one name, or a count.
  label: string;
  x: number;
  y: number;
}

export const dragState = reactive({
  drag: null as PaneDrag | null,
  /// The pane under the pointer, while dragging from inside or outside.
  over: null as PaneSide | null,
});

/// How far the mouse must move before a press turns into a drag.
const THRESHOLD = 5;

/// The pane at a point of the window, in CSS pixels.
export function paneAt(x: number, y: number): PaneSide | null {
  const pane = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-pane]');
  return (pane?.dataset.pane as PaneSide | undefined) ?? null;
}

/// Follows a press on a row: past the threshold it becomes a drag, and
/// releasing it over the other pane calls `onDrop`. `start` is called once,
/// when the drag begins, and gives what is dragged.
export function trackPress(
  down: MouseEvent,
  start: () => Omit<PaneDrag, 'x' | 'y'>,
  onDrop: (drag: PaneDrag) => void,
) {
  if (down.button !== 0) return;

  const move = (e: MouseEvent) => {
    if (!dragState.drag) {
      if (Math.hypot(e.clientX - down.clientX, e.clientY - down.clientY) < THRESHOLD) return;
      dragState.drag = { ...start(), x: e.clientX, y: e.clientY };
      document.documentElement.classList.add('pane-dragging');
    }
    dragState.drag.x = e.clientX;
    dragState.drag.y = e.clientY;
    dragState.over = paneAt(e.clientX, e.clientY);
  };

  const finish = (drop: boolean) => {
    const { drag, over } = dragState;
    window.removeEventListener('mousemove', move);
    window.removeEventListener('mouseup', up);
    window.removeEventListener('keydown', key, true);
    dragState.drag = null;
    dragState.over = null;
    document.documentElement.classList.remove('pane-dragging');
    if (drop && drag && over && over !== drag.from) onDrop(drag);
  };
  const up = () => finish(true);
  const key = (e: KeyboardEvent) => {
    if (e.key !== 'Escape' || !dragState.drag) return;
    e.preventDefault();
    e.stopPropagation();
    finish(false);
  };

  window.addEventListener('mousemove', move);
  window.addEventListener('mouseup', up);
  window.addEventListener('keydown', key, true);
}

/// The last part of a local path, with either separator.
export function baseName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
