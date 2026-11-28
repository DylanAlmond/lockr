# Findings

Prioritised findings from the renderer security audit. One section per sweep group; this document
fills in as the sweeps run.

Severity follows the map's scheme, not CVSS:

| | |
|---|---|
| **P0** | Renderer compromise leads to vault read, write, or exfiltration |
| **P1** | Secret exposure to a local observer, or vault-derived data leaving the machine |
| **P2** | Hardening gap that raises the likelihood of P0/P1 |
| **P3** | Hygiene |

Each finding follows `threat-model.md` §11: asset (`A#`), entry point (`E#`), adversary (`ADV#`),
rule broken, evidence, fix reference, status. Status vocabulary is in `evidence-standard.md` §3.

**State: partial.** Group 1 is complete; the remaining groups' findings are pending their sweeps.

---

## Group 1 — Configuration, capabilities and build settings

### F-01 — No Content Security Policy in either build mode · P2

```
A1–A5  ← E1  ADV1
Rule broken: threat-model R1 (no security property may depend on the renderer behaving)
```

`app.security.csp` is `null` and `devCsp` is unset. Tauri therefore injects no policy, so the
webview is the only thing preventing injected content from reaching the 30-command IPC surface. The
injection sweep found no HTML or script sink today, which is why this is P2 rather than P0 — but the
property is absent, not merely untested.

**Evidence:** `src-tauri/tauri.conf.json` (`"csp": null`); config schema defaults `devCsp` to the
production value, so dev mode is equally unprotected.
**Fix:** applied in `1b0b571`. `csp` and `devCsp` now carry the config-baseline policy, including
`img-src 'self' data:`, `object-src 'none'` and an IPC-only `connect-src`.
**Status:** fixed.

### F-02 — Dormant `opener` capability grants an unscoped filesystem surface · P2

```
A4, A5  ← E6  ADV1
Rule broken: least privilege (T-04, 13.2.2); ASVS 8.2.1
```

`opener:default` is granted to the main window although nothing in `src/` calls it. The set includes
`open_url` (any `http(s)`/`mailto`/`tel`) and `reveal_item_in_dir`, which has **no scope check at
all**: arbitrary existing-path reveal, a filesystem existence oracle, and UNC paths that trigger SMB
authentication on Windows.

**Evidence:** `src-tauri/capabilities/default.json`; `src-tauri/Cargo.toml:22`;
`src-tauri/src/lib.rs:44`; `research/opener-scope.md`.
**Fix:** applied in `bcce2f5`: the capability entry, Cargo dependency, plugin registration and npm
package are gone. Pinned by `static:capabilities-opener`.
**Status:** fixed.

### F-03 — `Object.prototype` is mutable in the webview · P2

```
A3, A4  ← E1, E2  ADV1, ADV2
Rule broken: prototype-pollution hardening (15.3.6, T-08)
```

`freezePrototype` is unset, so Tauri's default `false` applies. The renderer parses backend JSON into
plain objects; a polluted prototype is a persistence and logic-confusion vector if any sink is ever
introduced.

**Evidence:** config shows no `freezePrototype`; schema default is `false`.
**Fix:** applied in `bcce2f5` — `freezePrototype: true`. Pinned by `static:freeze-prototype`.
**Status:** fixed.

### F-04 — `core:default` and `core:window:default` over-grant the window · P2

```
A1–A5  ← E1  ADV1
Rule broken: least privilege (T-04, 8.2.1, 13.2.2)
```

The capability grants `core:default` (app, event, image, menu, path, resources, tray, webview,
window defaults) plus `core:window:default`, and then re-grants four window permissions already
included in that default. The app uses a handful: window controls, `isMaximized`, `onResized`, and
`getVersion`. Menu, tray, image, resources and path permissions are unused.

**Evidence:** `src-tauri/capabilities/default.json`; call sites in `src/components/layout/Titlebar.vue`
and `src/components/ui/AboutAppModal.vue`.
**Fix:** applied in `bcce2f5` — the eight-permission file from `tauri-config-baseline.md` §5, one per
call site (including `internal_toggle_maximize` for drag-region double-click). Pinned by
`static:capabilities`.
**Status:** fixed.

### F-05 — Commands are not scoped to a capability · P3

```
A1–A5  ← E1  ADV1
Rule broken: T-05 (command scoping)
```

`build.rs` is a plain `tauri_build::build()`, so all 30 commands are callable by the `main` window
with no command-level allowlist. Any renderer compromise reaches the whole surface. This is a
structural gap, but its blast radius is the same as F-01/F-04 until an injection sink exists.

**Evidence:** `src-tauri/build.rs`; `src-tauri/src/lib.rs:45-80`.
**Fix:** decide whether to adopt `AppManifest::commands` scoping. **Status:** open.

### F-06 — No security headers on protocol responses · P3

```
A3, A4  ← E1  ADV2
Rule broken: 3.4.4, T-16
```

`app.security.headers` is unset, so protocol responses carry no `X-Content-Type-Options: nosniff`,
`Cross-Origin-Resource-Policy`, `Cross-Origin-Opener-Policy` or `Permissions-Policy`.

**Evidence:** config shows no `headers`.
**Fix:** applied in `bcce2f5` — `nosniff`, CORP/COOP `same-origin` and a `Permissions-Policy`
denying device APIs. Pinned by `static:headers`.
**Status:** fixed.

### F-07 — Source maps are absent by accident, not by assertion · P3

```
A6  ← build  ADV4
Rule broken: 15.2.3, T-10
```

`dist/` contains no `*.map` and no `sourceMappingURL`, but `vite.config.ts` sets no
`build.sourcemap`, so the property holds only because Vite's default is `false`. A config change or
plugin could reverse it silently.

**Evidence:** `find dist -name '*.map'` → 0; no `sourcemap` key in `vite.config.ts`.
**Fix:** applied in `303b778` — `build.sourcemap` follows `TAURI_ENV_DEBUG`, so a release build cannot
ship maps. Pinned by `static:sourcemap-config`.
**Status:** fixed.

### F-08 — No documented inventory of allowed communication · P3

```
A4  ← E1, E4  ADV2
Rule broken: 13.1.1
```

The README claims "no network requests"; the bundle's only external string literals are
`http://www.w3.org` (SVG namespace) and `https://vuejs.org` (Vue error text). There is no written
inventory of permitted destinations, so the claim cannot be checked against a document.

**Evidence:** `README.md:23`; bundle host scan (`research/bundle-and-dependencies.md` §1).
**Fix:** add the egress allowlist to the security-level document. **Status:** open.

### Controls that held

- `dangerousDisableAssetCspModification` is absent (default `false`), so Tauri injection is intact.
- `withGlobalTauri` is unset (default `false`); the API is not on `window.__TAURI__`.
- Isolation is not enabled, but the decline is now recorded (`tauri-config-baseline.md` §6), which is
  what T-07 asks for.
- No token, seed fixture, source map or test code reaches the bundle.
- `devtools` is not a Cargo feature, so a release build should expose no inspector (not yet verified
  against a release artefact).

---

## Group 2 — Renderer injection and secret exposure

### F-09 — Vault-controlled icon URLs are fetched without a scheme allowlist · P1

```
A4, A5  ← E2, E4  ADV2
Rule broken: 1.2.2, 14.2.3, 13.2.4
```

An icon is an arbitrary backend string. Uploads are rasterised, but nothing validates the stored
value, and it is bound to `<img :src>` at nine sites. A tampered, imported or restored vault can set
`icon: "https://attacker.example/x.png"`; the request fires when the item is listed, with no action
beyond opening the app. That leaks the User's IP and user agent, and confirms to the attacker that
the vault was opened and which item was shown — vault-derived data leaving the machine.

`javascript:` is inert in `img.src` and SVG loaded through `<img>` does not execute, so this is not
script execution. It is egress.

**Evidence:** `ItemList.vue:248`, `AccountDetails.vue:199`, `CardDetails.vue:238`,
`IdentityDetails.vue:167`, `NoteDetails.vue:168`, `Sidebar.vue:131`, `LoginView.vue:91,131`,
`IconUpload.vue:58`; `vault_manager.rs:16-19,459-463` (only empty-string normalisation);
`csp: null` (F-01).
**Fix:** applied in `1b0b571`. The CSP's `img-src 'self' data:` blocks a remote icon fetch, and
`icons::is_allowed_icon` / `validate_icon` / `safe_icon` reject or withhold anything that is not a
raster `data:` image or a relative name, on both the write and the read path.
**Status:** fixed.

### F-10 — Revealed secrets never expire · P2

```
A3  ← E1  ADV1, ADV3
Rule broken: 14.3.1, P-03, R-S2
```

`AccountDetails` holds the decrypted password and `CardDetails` holds each card secret; both are
cleared only when the item id, `updated_at`, or the mutation counter changes. There is no timer, no
window-focus handler and no visibility handler — all ten `@blur` handlers are field-level re-masks,
not window-level. A revealed password therefore stays visible and reachable until the User navigates
or edits something.

**Evidence:** `AccountDetails.vue:125-140`; `CardDetails.vue:167`; zero
`addEventListener('blur'|'visibilitychange')` in `src/`; the only timer in the secret path is
`ItemField`'s two-second label cooldown.
**Fix:** applied in `97a6599`. `useSecretReveal` enforces a 10 s bound and hides every reveal on
window blur or document visibility, dropping the reference on hide and on unmount. Pinned by
`renderer:reveal-timeout` (`tests/compliance/secret-reveal.test.ts`).
**Status:** fixed.

### F-11 — Clipboard retains secrets indefinitely · P2

```
A3  ← E5  ADV3
Rule broken: P-04, R-S4
```

The only clipboard interaction is `navigator.clipboard.writeText`. The two-second timer resets the
"Copied" label and nothing else. The clipboard is not cleared on timeout, logout or exit, so any
process in the User's session can read the password long after the copy.

**Evidence:** `ItemField.vue:64-70`; no `clipboard.readText` or clear anywhere in `src/`.
**Fix:** applied in `97a6599`. `copy_item_secret` writes the secret in the core and never returns it
over IPC; the clipboard is cleared after 30 s only if it still holds that value, a newer copy keeps
its own window, and logout/window close compare-and-clear. Pinned by `rust:clipboard-expiry`. The
copier no longer fetches plaintext into the renderer, and the copy button now shows a 2 s tick as
feedback only.
**Status:** fixed.

### A rule refinement this sweep forced

P-06 originally said a mask never contains a character of the value. The card-number mask
deliberately shows the real last four digits (`secrets.ts:28`). The rule, not the product, was wrong:
R-S6 now permits the last four and nothing more, so the checklist no longer demands something the
design intentionally violates.

### Controls that held

- No HTML, script, navigation or `postMessage` sink exists in `src/` — zero hits across all six
  classes, corroborating the research.
- Uploaded images are rasterised through a canvas and re-encoded, so an SVG payload never renders as
  inline SVG.
- No `localStorage`, `sessionStorage` or IndexedDB use.
- No secret reaches the router, the query string, or a console call (the `seed.ts` fixture logs a
  test password, but it is not bundled).

---

## Group 3 — IPC and authorisation

### F-14 — Any vault file can be deleted by id, with no ownership, unlock or re-authentication · P0

```
A1–A5  ← E1  ADV1
Rule broken: 8.2.2, 8.3.1, P-01
```

`list_vault_ids` enumerates **every** `.vault` file on disk. `delete_vault` then removes the file for
any parsed UUID: it does not check that the vault belongs to the active User, does not require the
vault to be unlocked, and asks for no confirmation. `remove_vault_from_user` silently no-ops for ids
not in the active User's list, so the deletion is not even recorded against a profile.

A compromised renderer can therefore destroy every User's vault on the machine in one call per file.
This is the audit's only P0: write access to data outside the active session, reachable with no
precondition beyond the IPC surface itself.

**Evidence:** `commands.rs` `delete_vault`; `user_manager.rs:211-221`; `vault_manager.rs:364-374`;
`vault_manager.rs:102-125` (`list_vault_ids`).
**Fix:** applied in `98a35d1`. `delete_vault` now requires the vault in the active user's
`vault_ids`, re-verifies the master password against the profile, and moves the file to an
`<id>.vault.deleted` tombstone (ADR-0002) instead of unlinking it. Pinned by
`test_delete_vault_requires_the_master_password`,
`test_delete_vault_rejects_a_vault_the_active_user_does_not_own` and
`test_delete_vault_tombstones_an_owned_vault`.
**Status:** fixed.

### F-12 — No command checks vault ownership · P1

```
A3, A4  ← E1  ADV1
Rule broken: 8.2.2, 8.3.1, P-01; threat-model §8
```

`vault_ids` is documented in code as "IDs of all vaults this user owns" and is consulted exactly once:
in the login unlock loop. Every vault and item command gates on "present in the unlocked map"
(`vault_manager.rs:83-99`), never on "owned by the active User". `get_item_secret` additionally
validates the item and the field key, but not the caller.

Today only the active User's vaults are unlocked, so this is latent — it becomes a cross-User read
and write path the moment two Users' vaults are open at once, which F-13 makes possible, or if any
feature introduces concurrent sessions. It is a P1 rather than P0 because it needs that second
condition; F-14 does not.

**Evidence:** `commands.rs:148` (only `vault_ids` use); `vault_manager.rs:83-99`;
`commands.rs` `get_item_secret`.
**Fix:** applied in `0a9fb7d`. Every vault/item command calls
`ManagerState::require_owned_vault`; scan commands are scoped to the active user's vaults by
containing-vault id (`get_all_items`, `get_item_by_id`, `get_unlocked_vaults`, `list_vault_ids`), and
`flush_all`, `lock_vault`, `login_user` and `register_user` no longer reach across sessions.
**Status:** fixed.

### F-13 — Switching the active User does not end prior sessions · P2

```
A1–A3  ← E1  ADV1
Rule broken: 7.4.2, P-02; threat-model §7
```

`login_user` and `register_user` overwrite the cached master password and secret key without locking
what was already unlocked. `get_unlocked_vaults` and an unfiltered `get_all_items` then return every
unlocked vault, so the previous User's data stays readable in RAM.

The normal flow is safe because logout locks everything and the router only shows the login screen
when no User is active. The gap is reachable by a compromised renderer calling `login_user` directly,
and it is the precondition that turns F-12 into a cross-User path.

**Evidence:** `commands.rs` `login_user` (credentials overwritten before any lock);
`vault_manager.rs:128-133`; `commands.rs` `logout` (the only path that locks).
**Fix:** applied in `ed0afaf`, with failed-login ordering and the registration rollback pinned in
`6b4f297`. `ManagerState::end_session` flushes and locks every unlocked vault, zeroizes the cached
password and key, and clears the active user; `login_user` and `register_user` call it before setting
any credential. `logout` reuses the same path.
**Status:** fixed.

### F-15 — No anti-automation on login · P2

```
A1  ← E1  ADV1, ADV3
Rule broken: 2.4.1, 6.3.1
```

`login_user` has no attempt counter, delay, lockout or audit entry. Argon2id makes each guess
expensive, which is the only brake; a local attacker can retry indefinitely and nothing records it.

**Evidence:** `commands.rs` `login_user`; `user_manager.rs:110-132`.
**Fix:** applied in `7e6b85e`. Five consecutive failures block the next attempt for 5 minutes,
doubling to a 15-minute cap; success clears the counter. The counter and block are persisted in
`login-throttle.dat`, so a restart does not reset an active lockout, and the login screen counts down
per second from the core's remaining time. Attempts append `login_success`, `login_failed` and
`login_blocked` lines to `auth.log` (identifiers only). Pinned by `test_login_backoff_grows_and_caps`,
`test_login_throttle_blocks_and_clears`, `test_login_throttle_survives_restart` and
`rust:login-rate-limit`.
**Status:** fixed.

### F-16 — Validation and authorisation rules are undocumented · P3

```
A1–A5  ← E1  ADV1
Rule broken: 2.1.1, 2.3.2, 8.1.1
```

There is no written record of which command validates what, which fields a caller may set, or that
`vault_ids` is the access-control list. The knowledge exists only in the code, which is why F-12 and
F-14 were not obvious.

**Evidence:** no validation or authorisation document in `docs/`.
**Fix:** command→caller matrix and per-command validation rules.
**Status:** open.

### F-17 — No length bounds on strings crossing IPC · P3

```
A1–A5  ← E1  ADV1
Rule broken: 2.2.1 (partial)
```

Display names, tags, notes, URLs and secret values are accepted at unbounded length. The renderer
does not send dangerous lengths today, but the backend is the trusted layer and does not bound what
arrives.

**Evidence:** no length checks in `validation` paths of `vault_manager.rs`; `models.rs` uses `String`.
**Fix:** per-field maximum lengths, rejected with a validation error.
**Status:** open.

### Controls that held

- The safe-view pattern is real: `SafeItem`/`SafeVault` structurally omit secret values, and
  `get_item_secret` is the only egress.
- Field-level access is correct: the field key must be legal for the item kind
  (`vault_manager.rs:711-720`).
- `update_item` takes explicit named fields; ids, timestamps and `vault_ids` are not caller-settable,
  so mass assignment is not available.
- All validation that exists is in Rust, not in Vue.

---

## Group 4 — Authentication and session

### F-18 — No master-password policy at registration · P2

```
A1  ← E1  ADV3
Rule broken: 6.2.1, 6.2.4, 6.2.11
```

`register_user` accepts any non-empty master password. The strength meter exists but is called only
from the item editors (`AccountEdit`, `NewAccountForm`); registration never invokes it. The backend
`zxcvbn(&password, &[])` call also passes an empty user-input list, so product names and context
words are not penalised even where the meter does run.

The master password is the only factor protecting the vault, so a weak one is a weak vault regardless
of Argon2id. The KDF raises the cost per guess; it does not make a guessable password unguessable.

**Evidence:** `commands.rs` `register_user`; `commands.rs:330`;
`RegisterView.vue:27`; no strength call on the registration path.
**Fix:** applied in `7e6b85e`. `validate_master_password` requires 12 characters and a zxcvbn score
of 3+/4, with the user's name and product context words as inputs, and runs before the session
changes. The registration form shows the rule and disables submit below the length floor. Pinned by
`test_master_password_policy` and
`test_register_rejects_a_weak_password_without_ending_the_session`.
**Status:** fixed.

### F-19 — No idle or absolute session timeout · P2

```
A1–A3  ← E1  ADV3
Rule broken: 7.3.1, 7.3.2, 7.1.1
```

A session lasts from login until `logout` or process exit. `lock_vault` exists as a command but has
no renderer call site and no timer drives it. An unattended machine therefore leaves every vault
unlocked and every secret one reveal away.

**Evidence:** `commands.rs` `login_user` / `logout`; `lock_vault` registered at `lib.rs:61` with no
call site; no timer or inactivity listener anywhere in `src/`.
**Fix:** applied in `28d9c77` and recorded in `auth-policy.md`. The core enforces a 15-minute idle
timeout and a 12-hour absolute lifetime, tears the session down and emits `session-expired`; the
renderer reports activity and returns to auth.
**Status:** fixed.

### F-20 — No documented authentication or session policy · P3

```
A1  ← E1  ADV1
Rule broken: 6.1.1, 7.1.1
```

There is no written statement of the master-password rules, the lock semantics, or the intended
session lifetime. Without it, F-18 and F-19 cannot be judged as defects rather than choices.

**Evidence:** no auth or session document in `docs/` at audit time.
**Fix:** applied in `7e6b85e` — `auth-policy.md` covers the master-password rules, login throttling
and lock triggers, and session lifetime.
**Status:** fixed.

### Two standing deviations, not new findings

- **6.2.12 (breached-password check)** — a breach lookup needs a network call, which contradicts the
  local-first premise. Recorded in `security-level.md` §4 as a standing deviation.
- **6.3.3 (MFA)** — the only factor is the master password; the secret key is encrypted under it and
  is not independent possession. Recorded in `security-level.md` §4.1.

### Controls that held

- Password inputs are `type=password` on both auth screens, with an explicit reveal toggle.
- The exact password string is used for the KDF — no trimming or case folding.
- No forced rotation exists.
- `logout` flushes, locks every vault and zeroizes the cached password and key, so a terminated
  session is unusable (7.4.1).
- `delete_user` locks all vaults and zeroizes before deleting the profile, so account termination does
  end sessions (7.4.2). Profile *switching* is the separate F-13.

---

## Group 5 — Cryptography, files and data protection

### F-21 — Upload constraints are undocumented · P3

```
A6  ← E4  ADV2
Rule broken: 5.1.1
```

`accept="image/*"` plus a client-side `file.type.startsWith('image/')` check are the only stated
limits; there is no documented maximum size, dimension or format list. The transform bounds the
output (128×128, PNG/WebP), but the decode step accepts whatever the platform image decoder accepts,
and nothing records what that is.

**Evidence:** `imageUpload.ts:8,322`; `IconUpload.vue`; no upload spec in `docs/`.
**Fix:** document the accepted formats and a maximum input size, and enforce the size before decode.
**Status:** open.

### F-22 — No cryptographic inventory · P3

```
A1, A2  ← E2  ADV3
Rule broken: 11.1.2 (partial), 11.2.2 (partial)
```

The primitives are discoverable in `crypto.rs` — Argon2id v0x13 with m=64 MiB, t=3, p=4; AES-256-GCM
with a 12-byte nonce; a 32-byte salt; `OsRng` for salt, nonce and secret key. There is no document
that lists them with their keys and uses, and ADR-0001 covers schema versioning rather than an
inventory.

The file format is `Base64(salt ‖ nonce ‖ ciphertext)` with a `schema_version` but **no algorithm
identifier**. Changing the cipher therefore requires a schema migration, not a per-file tag. That is
a reasonable choice; it is recorded here rather than in a document a reviewer can find.

**Evidence:** `crypto.rs:43-57,95`; `vault_manager.rs` save/load; `ADR-0001`.
**Fix:** a crypto inventory section in the security-level document or its own `docs/` file.
**Status:** open. `static:deps-crypto` now asserts that `argon2` and `aes-gcm` resolve in the lockfile,
but the inventory document is still missing.

### F-23 — Per-tier protection requirements are incomplete · P3

```
A1–A6  ← all  ADV1–ADV3
Rule broken: 14.1.2 (partial)
```

The threat model classifies assets into T1/T2/T3 and `secret-lifetime.md` defines handling for T1
field-key secrets. There is no single statement of what each tier requires — encrypted at rest,
masked in the UI, never logged, retention and retention on lock — so a future asset has no template
to follow.

**Evidence:** `threat-model.md` §2; `secret-lifetime.md` §1.
**Fix:** a per-tier requirements table alongside the asset classification.
**Status:** open.

### Controls that held

- AES-256-GCM, an approved AEAD mode, with a fresh 12-byte nonce per encryption.
- Argon2id v0x13 at m=64 MiB, t=3, p=4 — above the OWASP minimum of m=19 MiB, t=2, p=1.
- `OsRng` for every non-guessable value: 32-byte salt, 12-byte nonce, 32-byte secret key.
- No MD5 or SHA-1 anywhere; Argon2id is the only password KDF.
- Every filesystem-touching id is a parsed `Uuid`; `vault_path()` formats it, and free-text fields
  never reach a path — structural traversal resistance, asserted rather than assumed.
- Uploaded images are decoded, rasterised and re-encoded, so a polyglot never reaches the vault as
  markup.
- The shipped app makes no external HTTP call, so there is no TLS surface to misconfigure.

The one naming wart: `derive_key` is marked "(old)" in `crypto.rs` but is the live path used to wrap
the secret key in the user profile. It is correct — password-only derivation is what encrypts the
secret key — but the comment is misleading.

---

## Group 6 — Supply chain and release hygiene

### F-24 — No SBOM and no dependency inventory · P3

```
A1–A5  ← build  ADV4
Rule broken: 15.1.2
```

The lockfile is the only record of the 388 resolved packages; there is no generated SBOM, no
committed inventory, and no script that produces one. An assessor cannot answer "what ships in this
build" from a document, only by resolving the lockfile themselves.

**Evidence:** no `cyclonedx`/`syft`/SBOM tooling in the tree; `package.json` has no audit or inventory
script.
**Fix:** generate an SBOM in the build, or commit one per release. **Status:** open.

### F-25 — A known-vulnerable dev dependency with no remediation path · P3

```
A6  ← build  ADV4
Rule broken: 15.2.1
```

`vite` 6.4.3 carries five advisories: one high (`launch-editor` `fs.existsSync` bypass on Windows),
three moderate (editor CLI flag misinterpretation, `%` escaping, files outside `server.fs.allow`),
and one low (cross-origin script execution through custom middleware). All are fixed by upgrading.
None ships to the User — `vite` is a devDependency — but there is no audit script, no CI gate and no
stated remediation timeframe, so the next advisory will be found the same way this one was: by
accident.

**Evidence:** dependency audit against the committed lockfile; `package.json` scripts contain no audit
step. **Fix:** applied in `303b778` — `vite` 6.4.3 → 6.4.6 and `vitest` 3.2.7 → 4.1.12 clear the
advisories; `yarn audit` gates high/critical and `dependency-policy.md` records the remediation window.
Pinned by `static:deps-audit`.
**Status:** fixed.

### F-26 — A dead network path and its token remain in source · P3

```
A2, A4  ← E4  ADV4
Rule broken: P-08 (source side), 13.3.1 (by margin)
```

`fetchBrandLogoAsBase64` still builds `https://img.logo.dev/name/<brand>?token=<VITE_LOGO_API_TOKEN>`
and `fetch()`es it, even though both call sites are commented out. The bundle is clean today because
Rollup tree-shakes the unreachable export — verified: 0 hits for the token or `logo.dev` in `dist/`.
But `VITE_*` values are inlined at build time wherever reachable code references them, so a single
re-enabled import, or a build change that keeps the module, puts both the token and the egress path
back in the bundle.

**Evidence:** `imageUpload.ts:263-310`; commented call sites at `AccountEdit.vue:130`,
`NewAccountForm.vue:8`; bundle scan clean.
**Fix:** applied in `303b778` — the function, the logo processor, the token reference and the
commented call sites are gone; there is no `.env` in the tree. Pinned by `static:dead-logo-path`.
**Status:** fixed.

### Controls that held

- `dist/` contains no seed fixture, no source map, no token and no dev-only module; 0 hits for
  `logo.dev`, `VITE_LOGO_API_TOKEN`, `Dev seed data` or `test12`.
- The only external string literals in the bundle are `http://www.w3.org` (SVG namespace) and
  `https://vuejs.org` (Vue error text).
- The lockfile resolves 388 packages with no git, URL, file or portal resolutions and no non-default
  registry.

---

## Group 7 — Logging and error handling

### F-28 — Vault writes are not atomic · P2

```
A1–A5  ← E2  ADV3 (crash/loss, not attacker)
Rule broken: 16.5.3
```

`save_vault` encrypts in memory and then calls `std::fs::write(path, encrypted)`, which truncates the
existing file before writing the new bytes. A crash, power loss or full disk during that window
leaves a truncated or empty vault. The `.OLD` backup is written only during a schema migration
(`vault_manager.rs:248`), not on ordinary saves, so it is not a recovery path for this.

The fix is the standard one: write to a sibling temp file, fsync, then rename over the target.

**Evidence:** `vault_manager.rs:292`; the only `fs::copy` is `backup_vault_file`, called from the
migration path at `:248`.
**Fix:** applied in `da46178`, recorded in ADR-0003: write `<id>.vault.tmp`, sync it, rename over the
live file. Pinned by `test_failed_save_leaves_the_existing_vault_intact`,
`test_failed_rename_keeps_the_existing_vault_and_cleans_up` (Windows) and
`test_save_leaves_no_temp_file_and_persists_the_new_content`. The Rust compliance suite
(`compliance-tests.md` §4) remains the planned home for the re-runnable assertion.
**Status:** fixed.

### F-27 — No logging of authentication, authorisation or errors · P3

```
A1–A5  ← E1  ADV1
Rule broken: 16.1.1, 16.3.1, 16.3.2, 16.3.4
```

There is no log sink. Successful and failed logins, authorisation denials (there is no authorisation
check to deny, F-12) and unexpected errors are all unrecorded. For a local-first app this is a
defensible choice, but it is currently an omission rather than a decision — there is no logging
policy saying so, and no way for a User to see that someone tried their master password.

**Evidence:** no `log`/`tracing` dependency; only two ad-hoc prints (`commands.rs:84`,
`user_manager.rs:59`).
**Fix:** a minimal local audit log, or a written decision that logging is deliberately absent plus
the consequence. **Status:** open. Authentication events are now recorded in `auth.log`
(`auth-policy.md` §2, F-15); authorisation denials and unexpected errors still are not, and there is
no logging policy.

### F-29 — Error strings and debug prints leak paths and identifiers · P3

```
A5  ← E1  ADV1
Rule broken: 16.5.1
```

`VaultError` derives `Display` from the underlying error. `Io(#[from] std::io::Error)` serialises as
`"IO error: <os message>"`, which includes the filesystem path, and serde errors likewise reach the
UI verbatim. Separately, `println!("id: {}", user_id)` prints a user identifier on every
`get_user` call, and `eprintln!` prints a profile path when a profile fails to parse.

None of these is a secret, but together they hand a renderer (or anyone reading a terminal) the app
data path and profile ids — useful reconnaissance for the filesystem-facing findings above.

**Evidence:** `error.rs:19-23`; `commands.rs:84`; `user_manager.rs:59`.
**Fix:** a UI-facing error type that carries a code and a generic message; delete the ad-hoc prints.
**Status:** open.

### Controls that held

- No secret is logged: there is no log sink, and the two prints carry an id and a path, not a value.
- `logout` zeroizes the cached password and key, and `delete_user` does the same before removing the
  profile.
- The migration path does take a backup before rewriting (`vault_manager.rs:248`).

---

## Complete

All seven sweep groups are recorded. Totals across 105 rows: **40 met, 17 partial, 41 deviated,
6 n/a, 1 unchecked**. The full set is 29 findings: one P0 (F-14), two P1 (F-09, F-12), and the rest
P2/P3.

The remaining unchecked row is T-15 (installer code signing), which needs a signed release artefact.
