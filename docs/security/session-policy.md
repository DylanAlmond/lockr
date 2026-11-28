# Session policy

Decision record for F-19 (idle and absolute session timeouts). It states the
rule, what ending a session does, and the limits of the control.

This is a session policy only. Master-password rules are covered separately
(F-18/F-20).

## 1. Lifetime

| Trigger | Value | Behaviour |
|---|---|---|
| Idle | **15 minutes** without activity | Session ends |
| Absolute | **12 hours** after login, regardless of activity | Session ends |

Activity is any vault/item command, plus a renderer heartbeat on
`pointerdown`, `keydown` or `wheel` (at most one report every 30 seconds).
The heartbeat is **advisory**: the core timer runs regardless and enforces
both limits even if the renderer never reports.

## 2. What ending a session does

The core tears the session down:

1. flushes dirty vaults (best-effort on a timeout; a disk error must not keep
   the session open);
2. locks every unlocked vault, dropping the cached master keys;
3. zeroizes the cached master password and secret key;
4. clears the clipboard if it still holds the last copied secret;
5. clears the active user and the session clock.

It then emits `session-expired`, and the renderer returns to the auth screen.
Resuming requires the master password; there is no separate locked-but-armed
state.

## 3. Why these choices

- **Backend enforcement.** A security property may not depend on the renderer
  behaving (threat-model R1). A compromised renderer can report false activity,
  but it cannot stop the core's absolute timeout, and the idle timer is driven
  by real commands as well.
- **Full logout, not a lock screen.** The app has no separate unlock flow, and
  a lock that leaves the master password cached in RAM would not bound the
  decrypted vaults. Ending the session is the control that does.
- **15 minutes** matches the default of comparable password managers and keeps
  an unattended machine from leaving secrets one reveal away.
- **12 hours** caps a session that stays active all day without forcing a
  re-login mid-task.

## 4. Limits

- **Activity is advisory.** A compromised renderer can extend the idle clock,
  but not the absolute clock.
- **Core plaintext is bounded, not erased.** The Rust core holds decrypted
  vaults for the whole session; the timeout closes the window, it does not
  scrub memory.
- **OS screen lock is separate.** This policy does not observe the OS lock
  state.
- **Flush is best-effort on timeout.** Unsaved changes may be lost if the disk
  is unavailable when the session expires; the alternative is keeping the
  session open.

## 5. Verification

1. Log in, leave the app untouched for 15 minutes: the auth screen returns and
   the core reports no unlocked vaults.
2. Interact at minute 14: the idle clock resets.
3. Keep a session active past 12 hours: it ends regardless of activity.
4. Copy a secret, let the session expire: the clipboard is cleared.
5. Inspect the session state after expiry: cached password and key are `None`.
