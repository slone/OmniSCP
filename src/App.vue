<script setup lang="ts">
import AppToolbar from './components/AppToolbar.vue';
import AppStatusBar from './components/AppStatusBar.vue';
import ProfileSidebar from './components/sidebar/ProfileSidebar.vue';
import DualPaneView from './components/filepane/DualPaneView.vue';
import BottomPanel from './components/bottom/BottomPanel.vue';
import FileActionDialogs from './components/dialogs/FileActionDialogs.vue';
</script>

<template>
  <div class="app-layout">
    <header class="app-toolbar">
      <AppToolbar />
    </header>

    <aside class="app-sidebar">
      <ProfileSidebar />
    </aside>

    <main class="app-main">
      <div class="main-files">
        <DualPaneView />
      </div>
      <div class="main-bottom">
        <BottomPanel />
      </div>
    </main>

    <footer class="app-statusbar">
      <AppStatusBar />
    </footer>

    <FileActionDialogs />
  </div>
</template>

<style>
:root {
  /* Colors — dark theme (the default); light overrides below */
  color-scheme: dark;
  --color-bg: #1e1e1e;
  --color-surface: #252526;
  --color-border: #3c3c3c;
  --color-hover: #2a2d2e;
  --color-active: #37373d;
  --color-selected: #094771;
  --color-accent: #0078d4;
  --color-accent-hover: #006cbd;
  --color-on-accent: #fff;
  --color-text: #cccccc;
  --color-text-muted: #808080;
  --color-overlay: rgba(0, 0, 0, 0.5);
  --shadow-color: rgba(0, 0, 0, 0.4);

  --color-success: #4ade80;
  --color-warning: #fbbf24;
  --color-danger: #f87171;
  --color-info: #60a5fa;
  --color-neutral: #9ca3af;
  --color-danger-solid: #b42318;
  --color-danger-solid-hover: #912018;
  --color-danger-border: #7f1d1d;
  --color-danger-text: #fca5a5;
  --bg-success: #1a3a1a;
  --bg-warning: #3a2f10;
  --bg-info: #1a2a3a;
  --bg-danger: #3a1a1a;
  --bg-neutral: #2a2a2a;

  /* Typography */
  --font-sans: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
  --font-mono: 'SF Mono', 'Cascadia Code', 'Fira Code', 'Consolas', monospace;

  /* Layout */
  --sidebar-width: 200px;
  --toolbar-height: 36px;
  --statusbar-height: 22px;
  --bottom-panel-height: 180px;

  font-family: var(--font-sans);
  font-size: 13px;
  line-height: 1.4;
  color: var(--color-text);
  background-color: var(--color-bg);

  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

:root[data-theme='light'] {
  color-scheme: light;
  --color-bg: #ffffff;
  --color-surface: #f3f3f3;
  --color-border: #d4d4d4;
  --color-hover: #e8e8e8;
  --color-active: #dcdcdc;
  --color-selected: #cce4f7;
  --color-accent: #0067c0;
  --color-accent-hover: #005ba8;
  --color-text: #1f1f1f;
  --color-text-muted: #6b6b6b;
  --color-overlay: rgba(0, 0, 0, 0.3);
  --shadow-color: rgba(0, 0, 0, 0.2);

  --color-success: #15803d;
  --color-warning: #b45309;
  --color-danger: #b91c1c;
  --color-info: #1d4ed8;
  --color-neutral: #6b7280;
  --color-danger-border: #fca5a5;
  --color-danger-text: #991b1b;
  --bg-success: #dcfce7;
  --bg-warning: #fef3c7;
  --bg-info: #dbeafe;
  --bg-danger: #fee2e2;
  --bg-neutral: #f3f4f6;
}

*,
*::before,
*::after {
  box-sizing: border-box;
  margin: 0;
  padding: 0;
}

html,
body,
#app {
  height: 100%;
  overflow: hidden;
}

/* As in a native application, the interface itself is not text to select:
   a selection started in a pane would spread to the toolbar and the rest. */
html {
  -webkit-user-select: none;
  user-select: none;
}

/* Text worth copying: fields, the log, error messages, properties. */
input,
textarea,
[role='alert'],
.log-entries,
.problem-row,
.facts dd,
.state-error {
  -webkit-user-select: text;
  user-select: text;
}

/* Read by screen readers, not shown. */
.visually-hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}

/* While files are dragged between the panes. */
.pane-dragging,
.pane-dragging * {
  cursor: copy !important;
}
</style>

<style scoped>
.app-layout {
  display: grid;
  grid-template-columns: var(--sidebar-width) 1fr;
  grid-template-rows: var(--toolbar-height) 1fr var(--statusbar-height);
  grid-template-areas:
    "toolbar  toolbar"
    "sidebar  main"
    "statusbar statusbar";
  height: 100%;
  overflow: hidden;
}

.app-toolbar {
  grid-area: toolbar;
}

.app-sidebar {
  grid-area: sidebar;
  overflow: hidden;
}

.app-main {
  grid-area: main;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.main-files {
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.main-bottom {
  height: var(--bottom-panel-height);
  flex-shrink: 0;
  overflow: hidden;
}

.app-statusbar {
  grid-area: statusbar;
}
</style>
