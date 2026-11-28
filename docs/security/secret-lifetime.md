# Renderer secret-lifetime and clipboard policy

Decision for [#71](https://github.com/DylanAlmond/lockr/issues/71) (parent [#63](https://github.com/DylanAlmond/lockr/issues/63)).
This is a policy, not a fix: it states the rule, compares it with current behaviour, and names the
gap in each component. Implementation becomes fix tickets after the map closes.

**Applied:** R-S2 and R-S4, including the backend copy path in §5, for the detail views, the Input
password toggle and the copier (F-10, F-11), and the idle/absolute session timeout (F-19,
`session-policy.md`). §3 describes the pre-fix behaviour. R-S3 (editor draft teardown) remains open.

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

**R-S6 — Masking invariant.** A display value is the structural mask until an explicit reveal. The
account-password mask and the security-code/PIN masks contain no character of the value. The one
deliberate exception is the card-number mask, which may expose the last four digits and no more;
that is the product's card UX, recorded rather than hidden. `ACCOUNT_PASSWORD_MASK` remains the copy
gate: a display value equal to it means "nothing real to copy". The strength meter may send plaintext
to the backend on keystroke — IPC is local and the backend already holds the plaintext — but its
result may never include the value.

**R-S7 — No false zeroization.** The renderer must not attempt to scrub a secret by overwriting or
rebinding it. A JavaScript string is immutable; `secret = 'x'.repeat(secret.length)` allocates a new
string and leaves the original intact until the GC frees — not zeroes — it. Engines copy strings
freely (concatenation, `slice`, `JSON.parse`, reactive proxying), the DOM keeps rendered values and
undo state, and Vue's reactivity caches derived strings, so no single reference owns the only copy.
Bounding lifetime and copies (R-S2, R-S3) is the achievable control. Any code that appears to erase
a string is a false assurance and should be rejected in review.

## 2. The limitation that must be stated publicly

JavaScript strings are immutable and cannot be zeroized. R-S2 and R-S3 drop the **reference**; they
do not scrub memory, and the string may survive in the webview heap until garbage collection. The
policy bounds *reachability* — what the UI and any injected script can ask for — not memory
residency.

The asymmetry is worth naming: the Rust core uses `zeroize` and that scrubbing is real; the renderer
cannot make the equivalent claim, and must not imply it. The public security-level document should
say exactly that rather than "secrets are wiped".

That is why the policy is paired with the threat model's session rule and the no-network CSP: an
attacker who can read webview memory has already defeated the renderer.

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
- **Copier** — copy should not fetch the value into the renderer at all; see §5. Until the backend
  command lands, record the copied value in a short-lived holder, start a 30 s timer, and
  compare-and-clear on expiry. Logout clears immediately.
- **App shell** — a single `blur` + `visibilitychange` listener that broadcasts "hide all reveals",
  rather than eight components each listening.

## 5. The copy path should leave the renderer

Copying is the most frequent secret interaction and the only one that needs no display. It should not
involve the webview.

**Target.** An app command `copy_item_secret(vault_id, item_id, field_key) -> Result<(), String>`
that reads the value from the decrypted vault in the core, writes it to the OS clipboard, and
returns only success. Nothing crosses IPC but a boolean.

What this removes:

- the plaintext never becomes a JavaScript string, so it never enters the webview heap at all;
- the 30 s compare-and-clear timer moves to Rust, where it survives route changes, component
  teardown and the window being backgrounded;
- `ItemField`'s copy path stops being a secret-fetch path, so `get_item_secret` serves reveal only.

Trade-offs: it needs a clipboard dependency in the core (`tauri-plugin-clipboard-manager` or
platform APIs) and the compare-and-clear read must also live there. Reveal still needs the string, so
this narrows the surface rather than eliminating it — which is exactly the point: it removes the
copy path from the renderer's attack surface entirely and leaves one irreducible reveal path.

## 6. Bound, not solved

This policy reduces the window in which a compromised or observed renderer can read a secret. It
does **not** stop ADV1 from calling `get_item_secret` directly: a renderer that is already executing
attacker code has full access to the command surface until the ownership check and session rules
land. Stated plainly: lifetime policy is exposure minimisation, not access control.

## 7. Out of scope

- **Core-side plaintext.** The Rust core holds decrypted vaults for the whole session, and
  `set_autosave(false)` can extend that indefinitely. Renderer copies are bounded here; core
  plaintext is bounded by the threat model's session rule.
- **Idle auto-lock.** Decided separately and implemented in `session-policy.md`: a 15-minute idle and
  12-hour absolute timeout end the session in the core. That is what bounds core plaintext; this
  policy continues to bound renderer reachability.
- **T2 content.** Note bodies, identity PII, usernames and URLs stay visible and copyable. The audit
  should record that "a note body is not a secret" is a product choice a User may not share.
- **OS clipboard history.** Clipboard managers may retain the value; outside app control, and stated
  in the public document.

## 8. Verification

1. Reveal a password; it re-masks after 10 s without input.
2. Reveal, then focus another window; it re-masks immediately. Repeat with the window minimised /
   document hidden.
3. Copy a password; after 30 s paste elsewhere — clipboard is empty.
4. Copy a password, immediately copy other text, wait 30 s — the other text survives.
5. Log out; clipboard is empty.
6. In a dev build, leave an editor, then inspect component state — both secret refs are `null`.
7. Inspect storage, the URL, and the console after a full session — no secret appears.

## 9. Checklist rows

Discharges or partly discharges: ASVS **14.3.1** (authenticated data cleared when the session ends),
**14.2.1** (no sensitive data in URLs), **16.2.5** (no secrets in logs), **7.4.1** (a terminated
session cannot be used again — with the session rule). **7.3.1** (inactivity timeout) is discharged
by `session-policy.md`. The project-specific rows are: reveal bound, clipboard expiry, never-hold
list, masking invariant, and no-false-zeroization.

## 10. Named follow-ups

Fix tickets after the map closes, in priority order:

1. **Backend clipboard command** — `copy_item_secret` in the core, with Rust-side compare-and-clear;
   removes the copy path from the renderer (§5).
2. **Shared reveal primitive** — one 10 s timer plus blur/visibility handling, used by both detail
   views and `Input`'s password toggle.
3. **Editor draft teardown** — drop `passwordOriginal`/`passwordInput` and card drafts on unmount and
   route change.
4. **Copy feedback is icon-only** — `ItemField` shows a 2 s tick when a copy succeeds. It is feedback,
   not a clipboard-expiry signal; the 30 s clear is enforced in the core.
5. **Idle auto-lock** — decided and implemented: a 15-minute idle and 12-hour absolute timeout end
   the session in the core (`session-policy.md`).
