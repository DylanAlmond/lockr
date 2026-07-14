import { ref, computed, type ComputedRef } from 'vue';
import type { Router, RouteLocationNormalized } from 'vue-router';

const stack = ref<string[]>([]);
const pointer = ref(-1);
let initialized = false;

let canGoBack: ComputedRef<boolean>;
let canGoForward: ComputedRef<boolean>;

export function useRouterHistory(router: Router) {
  if (!initialized) {
    initialized = true;

    const initialPath = router.currentRoute.value.fullPath;
    stack.value = [initialPath];
    pointer.value = 0;

    router.afterEach((to: RouteLocationNormalized) => {
      const path = to.fullPath;

      // If we're at the current pointer already, do nothing
      if (stack.value[pointer.value] === path) return;

      // Detect if this navigation matches the entry right BEFORE pointer (Going Back)
      if (pointer.value > 0 && stack.value[pointer.value - 1] === path) {
        pointer.value -= 1;
        return;
      }

      // Detect if this navigation matches the entry right AFTER pointer (Going Forward)
      if (pointer.value < stack.value.length - 1 && stack.value[pointer.value + 1] === path) {
        pointer.value += 1;
        return;
      }

      // Otherwise it's a "new" navigation — truncate any forward history
      stack.value = stack.value.slice(0, pointer.value + 1);
      stack.value.push(path);
      pointer.value = stack.value.length - 1;
    });

    canGoBack = computed(() => pointer.value > 0);
    canGoForward = computed(() => pointer.value < stack.value.length - 1);
  }

  function goBack() {
    if (canGoBack.value) router.go(-1);
  }

  function goForward() {
    if (canGoForward.value) router.go(1);
  }

  return { canGoBack, canGoForward, goBack, goForward };
}
