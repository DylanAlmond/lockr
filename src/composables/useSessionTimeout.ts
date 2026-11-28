import { onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useUser } from './useUser';

/** Don't spam the backend: one heartbeat per this window of user input. */
const HEARTBEAT_INTERVAL_MS = 30_000;
const ACTIVITY_EVENTS = ['pointerdown', 'keydown', 'wheel'] as const;

/**
 * The core owns the idle and absolute session timeouts. This reports user
 * input so the idle clock advances, and returns to the auth screen when the
 * core ends the session.
 */
export function useSessionTimeout() {
  const { handleSessionExpired } = useUser();

  let lastReported = 0;
  let unlisten: UnlistenFn | null = null;
  let disposed = false;

  function reportActivity() {
    const now = Date.now();
    if (now - lastReported < HEARTBEAT_INTERVAL_MS) return;

    lastReported = now;
    invoke('session_activity').catch(() => {
      // A failed heartbeat only means the idle clock does not advance here;
      // the core timeout remains in force.
    });
  }

  onMounted(async () => {
    for (const event of ACTIVITY_EVENTS) {
      window.addEventListener(event, reportActivity, { passive: true });
    }

    const stop = await listen('session-expired', () => {
      // The core already cleared the session; do not call logout (it expects
      // an active user). Just leave the authenticated UI.
      lastReported = 0;
      handleSessionExpired();
    });

    if (disposed) {
      stop();
    } else {
      unlisten = stop;
    }
  });

  onUnmounted(() => {
    disposed = true;

    for (const event of ACTIVITY_EVENTS) {
      window.removeEventListener(event, reportActivity);
    }

    unlisten?.();
  });
}
