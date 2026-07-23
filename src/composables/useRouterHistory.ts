import { computed, ref } from 'vue';
import type { Router } from 'vue-router';

export function useRouterHistory(router: Router) {
  // Trigger recomputation whenever navigation occurs
  const version = ref(0);

  router.afterEach(() => {
    version.value++;
  });

  const state = computed(() => {
    // Depend on version so this recomputes after each navigation
    version.value;
    console.log(window.history);

    return window.history.state ?? {};
  });

  const previous = computed(() => state.value.back ?? null);
  const current = computed(() => state.value.current ?? router.currentRoute.value.fullPath);
  const next = computed(() => state.value.forward ?? null);
  const position = computed(() => state.value.position ?? 0);
  const replaced = computed(() => state.value.replaced ?? false);

  const canGoBack = computed(
    () => state.value.position !== 0 && previous.value !== null && previous.value !== '/auth'
  );
  const canGoForward = computed(() => next.value !== null);

  function goBack() {
    if (canGoBack.value) {
      router.back();
    }
  }

  function goForward() {
    if (canGoForward.value) {
      router.forward();
    }
  }

  return {
    previous,
    current,
    next,
    position,
    replaced,
    canGoBack,
    canGoForward,
    goBack,
    goForward
  };
}
