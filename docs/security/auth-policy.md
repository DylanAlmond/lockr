# Authentication and session policy

Decision record for F-15 (login throttling), F-18 (master-password policy),
F-19 (session timeouts) and F-20 (the documented policy). It states the rules,
what the core enforces, and the limits of the controls.

## 1. Master password

The master password is the only authentication factor (behavioural MFA is a
recorded deviation, `security-level.md` §4.1).

At registration the core rejects a password that is either:

- shorter than **12 characters**, or
- scored below **3 of 4** by zxcvbn.

zxcvbn is given the user's name and the product context words (`lockr`, `vault`,
`password`) as user inputs, so a password built from those is penalised. The
check runs before any existing session is ended, so a rejected registration
leaves the current session intact.

There is no forced rotation, no expiry, and no breach-list lookup: a breach
lookup needs a network call, which contradicts the local-first premise.
Existing profiles keep their password; the policy applies when a profile is
created.

## 2. Login throttling

Failed logins are counted per user, in memory:

| Consecutive failures | Delay before the next attempt |
|---|---|
| 1–4 | none |
| 5 | 5 minutes |
| 6 | 10 minutes |
| 7, 8, … | doubles, capped at **15 minutes** |

A successful login clears the counter. The counter is persisted in
`login-throttle.dat` (JSON, identifiers and timestamps only) so restarting the
app does not clear an active block. It uses wall-clock time, so changing the
system clock can shorten or extend a block. The same counter and backoff cover
the re-authentication check before deleting a vault, so that path is not an
unthrottled password oracle. The login screen runs a live `m:ss` countdown from
the core's remaining time and disables the submit button while blocked.

Each attempt is appended to `auth.log` in the app data directory:

```
<timestamp> registered user=<id>
<timestamp> login_success user=<id>
<timestamp> login_failed user=<id>
<timestamp> login_blocked user=<id>
<timestamp> reauth_failed user=<id>
<timestamp> reauth_blocked user=<id>
```

The log contains identifiers and timestamps only, never a password, key, or
secret. It is best-effort: a logging failure never blocks authentication. An
attempt for an unknown user id returns before decryption and is not recorded;
only registered-user attempts are counted and logged.

## 3. Session lifetime

| Trigger | Value | Behaviour |
|---|---|---|
| Idle | **15 minutes** without activity | Session ends |
| Absolute | **12 hours** after login, regardless of activity | Session ends |

Activity is any vault/item command, plus a renderer heartbeat on
`pointerdown`, `keydown` or `wheel` (at most one report every 30 seconds).
The heartbeat is **advisory**: the core timer runs regardless and enforces
both limits even if the renderer never reports.

## 4. What ending a session does

The core tears the session down:

1. flushes dirty vaults (best-effort; a disk error must not keep the session
   open);
2. locks every unlocked vault, dropping the cached master keys;
3. zeroizes the cached master password and secret key;
4. clears the clipboard if it still holds the last copied secret;
5. clears the active user and the session clock.

It then emits `session-expired`, and the renderer returns to the auth screen.
Resuming requires the master password; there is no separate locked-but-armed
state.

## 5. Why these choices

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

## 6. Limits

- **Activity is advisory.** A compromised renderer can extend the idle clock,
  but not the absolute clock.
- **The throttle is a local file.** Deleting `login-throttle.dat` or moving the
  system clock can clear or shorten a block; a local actor with that access
  already has the vault files.
- **Lockout is a denial-of-service vector.** An attacker can delay a known
  user's login by failing repeatedly; the delay is capped at 15 minutes.
- **Core plaintext is bounded, not erased.** The Rust core holds decrypted
  vaults for the whole session; the timeout closes the window, it does not
  scrub memory.
- **OS screen lock is separate.** This policy does not observe the OS lock
  state.
- **Flush is best-effort when the session ends.** Unsaved changes may be lost
  if the disk is unavailable on logout or timeout; the alternative is keeping
  the session open.

## 7. Verification

1. Register with a password shorter than 12 characters or a predictable one:
   rejected with a validation error.
2. Fail five logins: the sixth is refused with a retry delay; the correct
   password also waits until the delay passes.
3. Inspect `auth.log`: the success/failure/blocked events are recorded.
4. Log in, leave the app untouched for 15 minutes: the auth screen returns and
   the core reports no unlocked vaults.
5. Keep a session active past 12 hours: it ends regardless of activity.
6. Copy a secret, let the session expire: the clipboard is cleared.
