<script setup lang="ts">
import { ref, onMounted, useId } from 'vue';
import BaseDialog from './dialogs/BaseDialog.vue';
import { t } from '../i18n';

const props = defineProps<{
  profileName: string;
  promptLabel: string;
  notice?: string;
  rememberDefault?: boolean;
}>();

const emit = defineEmits<{
  submit: [password: string, remember: boolean];
  cancel: [];
}>();

const password = ref('');
const remember = ref(props.rememberDefault ?? false);
const inputRef = ref<HTMLInputElement | null>(null);
const inputId = useId();

onMounted(() => {
  inputRef.value?.focus();
});

function onSubmit() {
  emit('submit', password.value, remember.value);
}
</script>

<template>
  <BaseDialog :title="t.auth.title(profileName)" :width="380" @cancel="emit('cancel')">
    <form class="form" @submit.prevent="onSubmit">
      <p v-if="notice" class="notice" role="alert">{{ notice }}</p>
      <div class="form-group">
        <label class="form-label" :for="inputId">{{ promptLabel }}</label>
        <input
          :id="inputId"
          ref="inputRef"
          v-model="password"
          type="password"
          class="form-input"
          autocomplete="off"
        />
      </div>
      <label class="checkbox-label">
        <input v-model="remember" type="checkbox" />
        {{ t.auth.remember }}
      </label>
    </form>
    <template #footer>
      <button class="btn" @click="emit('cancel')">{{ t.common.cancel }}</button>
      <button class="btn btn-primary" @click="onSubmit">{{ t.auth.submit }}</button>
    </template>
  </BaseDialog>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.notice {
  color: var(--color-warning);
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}
</style>
