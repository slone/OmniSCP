<script setup lang="ts">
import type { FileEntry } from '../../types';
import { t } from '../../i18n';

defineProps<{ entry: FileEntry }>();
</script>

<template>
  <span
    role="img"
    :aria-label="entry.is_symlink ? t.dialogs.link : entry.is_dir ? t.dialogs.folder : t.dialogs.file"
    >{{ entry.is_symlink ? '🔗' : entry.is_dir ? '📁' : '📄' }}</span
  >
</template>
