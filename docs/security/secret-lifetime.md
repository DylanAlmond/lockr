# Renderer secret-lifetime and clipboard policy

Decision for [#71](https://github.com/DylanAlmond/lockr/issues/71) (parent [#63](https://github.com/DylanAlmond/lockr/issues/63)).
This is a policy, not a fix: it states the rule, compares it with current behaviour, and names the
gap in each component. Implementation becomes fix tickets after the map closes.

Scope: **T1 field-key secrets** from the threat model — account `password`; card `card_number`,
`security_code`, `pin`. Master-password reveal toggles on the auth screens are included because
they expose A1 by the same mechanism. Note bodies and identity PII are T2 and are not covered here
(see §6).

## 1. The rule

**R-S1 — Entry.** A secret enters the renderer only through an explicit reveal or an explicit copy.
Nothing else may fetch one as a side effect.

**R-S2 — Reveal bound.** A revealed secret re-masks after **10 seconds**, and immediately when the
window loses focus or the document becomes hidden — whichever comes first. Hiding **drops the
reference** (sets it to null), so the next reveal fetches afresh from the backend.

**R-S3 — Editor drafts.** A secret in an editor lives only while that form is mounted. On unmount,
route change, or item change, the draft and its original are both dropped; the policy does not wait
for `updated_at` or a save.

**R-S4 — Clipboard.** After **30 seconds**, read the clipboard; if it still equals the secret that
was copied, overwrite it with an empty string; if it has changed or the read fails, leave it alone.
Clear the clipboard on **logout**, and best-effort on window close. Never clobber content the User
copied afterwards.

**R-S5 — Never-hold list.** No secret in `appStore`, router params or query, `localStorage`,
`sessionStorage`, IndexedDB, the URL, logs, error strings, `aria-label`/`title`/placeholder
attributes, or any telemetry.

**R-S6 — Masking invariant.** A display value is the structural mask until an explicit reveal, and
the mask never contains any character of the value. `ACCOUNT_PASSWORD_MASK` remains the copy gate:
a display value equal to it means "nothing real to copy". The strength meter may send plaintext to
the backend on keystroke — IPC is local and the backend already holds the plaintext — but its result
may never include the value.

## 2. The limitation that must be stated publicly

JavaScript strings are immutable and cannot be zeroized. R-S2 and R-S3 drop the **reference**; they
do not scrub memory, and the string may survive in the webview heap until garbage collection. The
policy bounds *reachability* — what the UI and any injected script can ask for — not memory
residency.

That is why the policy is paired with the threat model's session rule and the no-network CSP: an
attacker who can read webview memory has already defeated the renderer. The public security-level
document must say this rather than imply that "secrets are wiped".

## 3. Current behaviour against the rule

| Surface | Today | Gap |
|---|---|---|
| Account detail reveal (`AccountDetails.vue`) | `password` ref cached; `showPassword` reset only on item id / `updated_at` / mutation count (`:125-140`) | No time bound, no blur/visibility bound; value not dropped |
| Card detail reveal (`CardDetails.vue`) | Per-field `value` + `shown`; cleared on active item change (`:167`) | Same |
| Account editor (`AccountEdit.vue`) | `passwordOriginal` **and** `passwordInput` held for the form's life; refetch on focus | Cleared on item change only; no unmount/route drop; two copies instead of one |
| Card editor (`CardEdit.vue`) | Focus fetches the stored value; blur re-masks (`:100-104`) | Value stays in the field model after blur |
| New-item forms | Focus reveals, blur re-masks (`NewAccountForm`, `NewCardForm`) | Same |
| Copy (`ItemField.vue`) | `navigator.clipboard.writeText`; 2 s "Copied" label cooldown (`:67-70`) | **Clipboard never cleared**; the 2 s timer is cosmetic only |
| Password reveal toggles (`Input.vue`) | Toggle on/off, no timer | No bound |
| Window focus / visibility | No listener anywhere in `src/` | Entire rule absent |
| Storage / URL / logs | No `localStorage`, `sessionStorage`, IndexedDB, or secret-in-query | Holds today; must be asserted in the checklist |

## 4. What changes per surface

- **Detail views** — one shared reveal primitive: start a 10 s timer on reveal, cancel on hide,
  clear on `blur`/`visibilitychange`, and null the value when hiding. Both detail panels and
  `Input`'s password toggle use it, so the behaviour cannot drift per component.
- **Editors** — drop `passwordOriginal`/`passwordInput` and card field drafts on `onUnmounted` and
  on route change. Keeping the original while the form is open is what makes "leave the password
  untouched" safe; it has no reason to outlive the form.
- **Copier** — record the copied value in a short-lived holder, start a 30 s timer, and
  compare-and-clear on expiry. Logout clears immediately. The holder is dropped as soon as the
  timer fires.
- **App shell** — a single `blur` + `visibilitychange` listener that broadcasts "hide all reveals",
  rather than eight components each listening.

## 5. Bound, not solved

This policy reduces the window in which a compromised or observed renderer can read a secret. It
does **not** stop ADV1 from calling `get_item_secret` directly: a renderer that is already executing
attacker code has full access to the command surface until the ownership check and session rules
land. Stated plainly: lifetime policy is exposure minimisation, not access control.

## 6. Out of scope

- **Core-side plaintext.** The Rust core holds decrypted vaults for the whole session, and
  `set_autosave(false)` can extend that indefinitely. Renderer copies are bounded here; core
  plaintext is bounded by the threat model's session rule.
- **Idle auto-lock.** The app has no inactivity timeout. That is a finding under ASVS 7.3.1, not a
  parameter of this policy, and it needs its own decision about lock semantics.
- **T2 content.** Note bodies, identity PII, usernames and URLs stay visible and copyable. The audit
  should record that "a note body is not a secret" is a product choice a User may not share.
- **OS clipboard history.** Clipboard managers may retain the value; outside app control, and stated
  in the public document.

## 7. Verification

1. Reveal a password; it re-masks after 10 s without input.
2. Reveal, then focus another window; it re-masks immediately. Repeat with the window minimised /
   document hidden.
3. Copy a password; after 30 s paste elsewhere — clipboard is empty.
4. Copy a password, immediately copy other text, wait 30 s — the other text survives.
5. Log out; clipboard is empty.
6. In a dev build, leave an editor, then inspect component state — both secret refs are `null`.
7. Inspect storage, the URL, and the console after a full session — no secret appears.

## 8. Checklist rows

Discharges or partly discharges: ASVS **14.3.1** (authenticated data cleared when the session ends),
**14.2.1** (no sensitive data in URLs), **16.2.5** (no secrets in logs), **7.4.1** (a terminated
session cannot be used again — with the session rule). Leaves **7.3.1** (inactivity timeout) open as
a finding, and adds the project-specific rows: reveal bound, clipboard expiry, never-hold list,
masking invariant.
