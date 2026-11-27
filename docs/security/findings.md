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
**Fix:** `tauri-config-baseline.md` §1. **Status:** fix-specified.

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
**Fix:** remove the capability, plugin and package (`tauri-config-baseline.md` §4). **Status:**
fix-specified.

### F-03 — `Object.prototype` is mutable in the webview · P2

```
A3, A4  ← E1, E2  ADV1, ADV2
Rule broken: prototype-pollution hardening (15.3.6, T-08)
```

`freezePrototype` is unset, so Tauri's default `false` applies. The renderer parses backend JSON into
plain objects; a polluted prototype is a persistence and logic-confusion vector if any sink is ever
introduced.

**Evidence:** config shows no `freezePrototype`; schema default is `false`.
**Fix:** `tauri-config-baseline.md` §1. **Status:** fix-specified.

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
**Fix:** the seven-permission file in `tauri-config-baseline.md` §5. **Status:** fix-specified.

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
**Fix:** `tauri-config-baseline.md` §1. **Status:** fix-specified.

### F-07 — Source maps are absent by accident, not by assertion · P3

```
A6  ← build  ADV4
Rule broken: 15.2.3, T-10
```

`dist/` contains no `*.map` and no `sourceMappingURL`, but `vite.config.ts` sets no
`build.sourcemap`, so the property holds only because Vite's default is `false`. A config change or
plugin could reverse it silently.

**Evidence:** `find dist -name '*.map'` → 0; no `sourcemap` key in `vite.config.ts`.
**Fix:** `tauri-config-baseline.md` §7. **Status:** fix-specified.

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
**Fix:** `img-src 'self' data:` (config baseline §2) plus backend scheme validation.
**Status:** fix-specified.

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
**Fix:** `secret-lifetime.md` R-S2. **Status:** fix-specified.

### F-11 — Clipboard retains secrets indefinitely · P2

```
A3  ← E5  ADV3
Rule broken: P-04, R-S4
```

The only clipboard interaction is `navigator.clipboard.writeText`. The two-second timer resets the
"Copied" label and nothing else. The clipboard is not cleared on timeout, logout or exit, so any
process in the User's session can read the password long after the copy.

**Evidence:** `ItemField.vue:64-70`; no `clipboard.readText` or clear anywhere in `src/`.
**Fix:** `secret-lifetime.md` R-S4, plus the backend copy path in §5. **Status:** fix-specified.

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
**Fix:** add the ownership check and a re-authentication step; move the file aside (the `.deleted`
pattern ADR-0002 once proposed does not exist) instead of unlinking.
**Status:** open.

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
**Fix:** assert `vault_id ∈ active_user.vault_ids` in every vault/item command.
**Status:** open.

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
**Fix:** end every session at the start of `login_user`/`register_user`.
**Status:** open.

### F-15 — No anti-automation on login · P2

```
A1  ← E1  ADV1, ADV3
Rule broken: 2.4.1, 6.3.1
```

`login_user` has no attempt counter, delay, lockout or audit entry. Argon2id makes each guess
expensive, which is the only brake; a local attacker can retry indefinitely and nothing records it.

**Evidence:** `commands.rs` `login_user`; `user_manager.rs:110-132`.
**Fix:** a counter with backoff, plus an auth log (group 7).
**Status:** open.

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

## Pending

Groups 4–7: authentication and session; cryptography, files and data protection; supply chain and
release hygiene; logging and error handling.
