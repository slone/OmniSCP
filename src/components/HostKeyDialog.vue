<script setup lang="ts">
import { ref, onMounted } from 'vue';
import BaseDialog from './dialogs/BaseDialog.vue';
import { t } from '../i18n';

defineProps<{
  profileName: string;
  host: string;
  port: number;
  fingerprint: string;
  /// FTPS: a TLS certificate rather than an SSH host key.
  certificate?: boolean;
}>();

const emit = defineEmits<{
  accept: [];
  cancel: [];
}>();

const cancelRef = ref<HTMLButtonElement | null>(null);

onMounted(() => {
  // Focus the safe choice so Enter does not trust the key by accident.
  cancelRef.value?.focus();
});
</script>

<template>
  <BaseDialog :title="t.hostKey.title(profileName)" :width="440" @cancel="emit('cancel')">
    <p>{{ certificate ? t.hostKey.certificateIntro(`${host}:${port}`) : t.hostKey.intro(`${host}:${port}`) }}</p>
    <div class="fingerprint" :aria-label="certificate ? t.hostKey.certificateFingerprint : t.hostKey.fingerprint">
      {{ fingerprint }}
    </div>
    <p>{{ t.hostKey.remember }}</p>
    <template #footer>
      <button ref="cancelRef" class="btn" @click="emit('cancel')">{{ t.common.cancel }}</button>
      <button class="btn btn-primary" @click="emit('accept')">{{ t.hostKey.accept }}</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.fingerprint {
  padding: 8px;
  border: 1px solid var(--color-border);
  border-radius: 3px;
  background: var(--color-bg);
  font-family: var(--font-mono);
  font-size: 12px;
  word-break: break-all;
  user-select: all;
}
</style>
