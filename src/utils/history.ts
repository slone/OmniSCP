import { computed, ref } from 'vue';

export type NavKind = 'new' | 'back' | 'forward';

const MAX_ENTRIES = 100;

/// Back/forward stacks of a pane. The store calls `moved` only once a folder was
/// listed, so a failed navigation leaves the history as it was.
export function useNavHistory() {
  const back = ref<string[]>([]);
  const forward = ref<string[]>([]);

  function moved(from: string, to: string, kind: NavKind) {
    if (!from || from === to) return;
    if (kind === 'back') {
      back.value.pop();
      forward.value.push(from);
    } else if (kind === 'forward') {
      forward.value.pop();
      back.value.push(from);
    } else {
      back.value.push(from);
      if (back.value.length > MAX_ENTRIES) back.value.shift();
      forward.value = [];
    }
  }

  function reset() {
    back.value = [];
    forward.value = [];
  }

  return {
    canGoBack: computed(() => back.value.length > 0),
    canGoForward: computed(() => forward.value.length > 0),
    previous: () => back.value[back.value.length - 1] as string | undefined,
    next: () => forward.value[forward.value.length - 1] as string | undefined,
    moved,
    reset,
  };
}
