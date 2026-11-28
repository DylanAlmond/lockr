import { onUnmounted, ref } from 'vue';

/** Format a second count as `m:ss` (e.g. 300 → "5:00"). */
export function formatCountdown(totalSeconds: number): string {
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const minutes = Math.floor(seconds / 60);

  return `${minutes}:${String(seconds % 60).padStart(2, '0')}`;
}

/** A per-second countdown for the login throttle. The core owns the deadline. */
export function useLoginCooldown() {
  const remaining = ref(0);
  let timer: ReturnType<typeof setInterval> | null = null;

  function stop() {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  }

  function begin(seconds: number) {
    stop();
    remaining.value = Math.max(0, Math.floor(seconds));

    if (remaining.value === 0) return;

    timer = setInterval(() => {
      remaining.value = Math.max(0, remaining.value - 1);
      if (remaining.value === 0) stop();
    }, 1_000);
  }

  function clear() {
    stop();
    remaining.value = 0;
  }

  onUnmounted(stop);

  return { remaining, begin, clear };
}
