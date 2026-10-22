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
  /* Colors — dark theme */
  --color-bg: #1e1e1e;
  --color-surface: #252526;
  --color-border: #3c3c3c;
  --color-hover: #2a2d2e;
  --color-active: #37373d;
  --color-accent: #0078d4;
  --color-text: #cccccc;
  --color-text-muted: #808080;

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
