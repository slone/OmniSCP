// Sizes of the window's areas set by the user, kept in this browser's storage.

import { reactive } from 'vue';

export const SIDEBAR = { min: 160, max: 480, default: 200 };
/// The local pane's share of the room the two file panes have.
export const PANE_SPLIT = { min: 0.2, max: 0.8, default: 0.5 };

const KEY = 'omniscp.layout';

function load() {
  const layout = { sidebar: SIDEBAR.default, paneSplit: PANE_SPLIT.default };
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    const within = (v: unknown, { min, max }: { min: number; max: number }) =>
      typeof v === 'number' && v >= min && v <= max;
    if (within(saved.sidebar, SIDEBAR)) layout.sidebar = saved.sidebar;
    if (within(saved.paneSplit, PANE_SPLIT)) layout.paneSplit = saved.paneSplit;
  } catch {
    // Defaults, then.
  }
  return layout;
}

export const layout = reactive(load());

export function saveLayout() {
  try {
    localStorage.setItem(KEY, JSON.stringify(layout));
  } catch {
    // Not worth more than losing the sizes at the next start.
  }
}
