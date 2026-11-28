# Logging policy

What Lockr records, what it deliberately does not, and the consequence.
Decision record for F-27.

## 1. Authentication log

`auth.log` in the app data directory records one line per event:

```
<timestamp> registered user=<id>
<timestamp> login_success user=<id>
<timestamp> login_failed user=<id>
<timestamp> login_blocked user=<id>
<timestamp> reauth_failed user=<id>
<timestamp> reauth_blocked user=<id>
```

- Contains identifiers and timestamps only — never a password, key, secret,
  item name or filesystem path.
- Append-only, best-effort: a logging failure never blocks authentication.
- The User can read it to see that someone tried their master password.
- An attempt for an unknown user id returns before decryption and is not
  recorded; only registered-user attempts are counted and logged.

## 2. What is not logged

| Event | Why not |
|---|---|
| Item reads, writes, deletes, moves | No log sink; the vault file is the record |
| Authorisation denials (ownership/throttle) | The denial is returned to the caller; logging it would add a second identifier store |
| Unexpected errors (I/O, serialization) | The UI gets a generic message; the path/parser detail stays in `Debug` and is not written anywhere |
| IPC payloads | Would risk secret values |
| Telemetry / analytics | None; the app makes no outbound request |

## 3. Consequences

- **No post-incident forensics beyond authentication attempts.** If a vault is
  tampered with on disk, the app does not record when or by whom.
- **No operational error log.** A failed save surfaces to the User; a failure
  after the UI has moved on is not retained.
- **The log is local and unencrypted.** It contains user ids and timestamps,
  which are T2 metadata; it must not be added to a backup or sync path without
  review.

These are accepted for a local-first, single-user-at-a-time desktop app. If a
sync, updater or multi-user feature lands, this decision must be revisited.

## 4. Retention

The file grows with authentication events and is not rotated. It is small
(hundreds of bytes per event) and lives with the profiles; deleting it resets
the visible record and is outside the app's control.
