import { computed, onUnmounted, ref, type Ref } from 'vue';

/** A revealed secret re-masks after this long (secret-lifetime R-S2). */
export const REVEAL_TTL_MS = 10_000;

type HideFn = () => void;

// One global blur/visibility listener broadcasts "hide every reveal" instead
// of every field installing its own listener.
const hiders = new Set<HideFn>();
let listening = false;
let windowHidden = false;

function hideAll() {
  for (const hide of [...hiders]) hide();
}

function bindGlobalListeners() {
  if (listening) return;
  listening = true;

  window.addEventListener('blur', () => {
    windowHidden = true;
    hideAll();
  });

  window.addEventListener('focus', () => {
    windowHidden = false;
  });

  document.addEventListener('visibilitychange', () => {
    windowHidden = document.hidden;
    if (document.hidden) hideAll();
  });
}

export interface SecretReveal {
  /** The revealed value; null while masked. */
  secret: Ref<string | null>;
  isRevealed: Ref<boolean>;
  /** Fetch (if needed) and reveal; the value is dropped when hiding. */
  reveal: (fetchValue: () => Promise<string | null>) => Promise<void>;
  /** Hide if revealed, otherwise reveal. */
  toggle: (fetchValue: () => Promise<string | null>) => Promise<void>;
  hide: () => void;
}

/**
 * Bound the lifetime of a revealed secret.
 *
 * The value is dropped after `ttlMs` and immediately when the window loses
 * focus or the document becomes hidden. Dropping the reference is the
 * achievable control in JavaScript; it does not zero memory (R-S7).
 */
export function useSecretReveal(ttlMs = REVEAL_TTL_MS): SecretReveal {
  const secret = ref<string | null>(null);
  const isRevealed = computed(() => secret.value !== null);

  let timer: ReturnType<typeof setTimeout> | null = null;
  let generation = 0;

  function clearTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function hide() {
    // Invalidate any fetch still in flight so it cannot reveal after hiding.
    generation += 1;
    clearTimer();
    secret.value = null;
  }

  async function reveal(fetchValue: () => Promise<string | null>) {
    if (secret.value !== null) return;

    const expected = ++generation;
    const fetched = await fetchValue();

    if (expected !== generation || windowHidden || fetched === null) return;

    secret.value = fetched;
    clearTimer();
    timer = setTimeout(hide, ttlMs);
  }

  async function toggle(fetchValue: () => Promise<string | null>) {
    if (secret.value !== null) {
      hide();
      return;
    }

    await reveal(fetchValue);
  }

  bindGlobalListeners();
  hiders.add(hide);

  onUnmounted(() => {
    hiders.delete(hide);
    hide();
  });

  return { secret, isRevealed, reveal, toggle, hide };
}
