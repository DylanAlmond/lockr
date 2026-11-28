# Audit checklist

The repeatable instrument for the renderer security audit. Evidence for
[#73](https://github.com/DylanAlmond/lockr/issues/73) (parent [#63](https://github.com/DylanAlmond/lockr/issues/63)).

It contains **no findings**. A row is a question; the answer goes in `findings.md` and the row's
evidence cell. Run it against a pinned commit, and the diff between two runs is the regression report.

Grouped by **what you open to check it**, not by ASVS chapter: one code path often discharges rows
from several chapters, and chapter-grouping would make the sweep read the same file repeatedly. The
ASVS/Tauri id on each row preserves the standard mapping.

## How to run it

1. Pin the commit. Record it in the run header (§0).
2. Run the automated checks first: `yarn build` (the bundle checks need `dist/`), then
   `yarn test:compliance`, `cargo test` in `src-tauri`, and `yarn test`. Fill status and evidence
   for every row with a `test-id`.
3. Walk the `manual:` rows in group order, recording what was inspected.
4. A row is never left blank. `n/a` requires a reason in Notes.
5. Findings go to `findings.md` with the citation format in `threat-model.md` §11; the checklist row
   links to the finding id.

**Status key:** ⬜ unchecked · ✅ met · ⚠️ partial · ❌ deviated · ➖ n/a · 🛠️ fixed.
Definitions in `evidence-standard.md` §3. A deliberately retained deviation (the `accepted` status)
is marked ❌ with `accepted` named in Notes; there is no separate emoji.

**Verification ids:** `static:<slug>` (scripts over config, source, or built artefact) ·
`rust:<slug>` (`src-tauri/tests/`) · `renderer:<slug>` (vitest) · `manual:<reason>`.
The rule: every row has one, and `manual:` states why automation is not possible.

## 0. Run header

| Field | Value |
|---|---|
| Commit | `a41cde1` app code (all groups; full `a41cde1cc794de4ab3cf6f52349a823ad9a90d52`) |
| Date | 2026-11-28 |
| Runner | OpenCode subagent |
| Toolchain | Node 24.7.0 · Rust 1.99.1 stable · Tauri 2.11.4 |
| Automated checks | `yarn test:compliance` 31/31 static passed (after `yarn build`) · `cargo test` 104 passed (101 unit + 3 integration) · `yarn test` 13 passed (3 files) · `yarn build` succeeded |
| Release artefacts | Unsigned MSI + NSIS at `bdea025`; both `NotSigned` (`release-verification.md`). T-15 not met; T-09/13.4.1/13.4.2 still need a launch/payload check |
| Rows (✅/⚠️/❌/➖/⬜) | g1 18/0/0/3/2 · g2 22/1/0/1/0 · g3 13/0/0/0/0 · g4 13/0/2/0/0 · g5 12/1/0/2/0 · g6 6/0/0/0/2 · g7 5/0/2/0/0 — **total ✅ 89 · ⚠️ 2 · ❌ 4 · ➖ 6 · ⬜ 4** |

---

## 1. Configuration, capabilities and build settings

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| T-01 | `app.security.csp` is set; no `unsafe-eval`/`unsafe-inline` for scripts | `static:csp` | ✅ | `default-src 'self'`, `script-src 'self'`, `img-src 'self' data:`, `object-src 'none'`; no unsafe sources (F-01, `1b0b571`) |
| T-02 | `devCsp` set separately; dev relaxations never ship | `static:csp-dev` | ✅ | `devCsp` set; script/object/base/frame/form equal production; only dev HMR `connect-src` and `style-src 'unsafe-inline'` differ (F-01) |
| T-03 | `dangerousDisableAssetCspModification` absent/false, so Tauri injects nonces | `static:csp-injection` | ✅ | Absent → default `false`; injection enabled |
| T-04 | Capabilities are least privilege; no cross-window merge | `static:capabilities` | ✅ | Eight-permission core set, one per call site; `core:default` and duplicate window grants removed (F-04, `bcce2f5`) |
| T-05 | Commands are scoped per window/capability, not all-to-all | `static:command-scope` | ✅ | 33 `#[tauri::command]` = `build.rs` `AppManifest::commands` = capability `allow-*` (F-05, `a29b13f`) |
| T-06 | IPC args deserialise into concrete types and are validated | `static:command-validation` | ✅ | No `serde_json::Value`; typed structs plus `validation.rs` bounds, enforced in Rust |
| T-07 | Isolation pattern used, or its absence recorded as a decision | `manual:declined in tauri-config-baseline.md §6` | ✅ | Declined and recorded; `brownfield` default |
| T-08 | `freezePrototype` is `true` | `static:freeze-prototype` | ✅ | `freezePrototype: true` (F-03, `bcce2f5`) |
| T-09 | Devtools unavailable in release builds | `manual:release build, attempt inspector` | ⬜ | `static:devtools-absent` confirms no `devtools` Cargo feature, but no release artefact was available to launch in this run |
| T-10 | No source maps in the release bundle | `static:sourcemap-config` + `static:bundle-sourcemaps` | ✅ | Maps tied to `TAURI_ENV_DEBUG`; built `dist/` has 0 `.map` files and no `sourceMappingURL` (F-07, `303b778`) |
| T-11 | `withGlobalTauri` is `false` | `static:config-global-tauri` | ✅ | Unset → default `false` |
| T-12 | Single-instance plugin first, `argv`/`cwd` treated as untrusted | `manual:not present in 1.2.0` | ➖ | No single-instance plugin; row stays for a future addition |
| T-13 | Deep links statically configured; URL shape validated | `manual:not present in 1.2.0` | ➖ | No deep-link plugin |
| T-14 | Updater artifacts signed, key pinned, HTTPS endpoints | `manual:not present in 1.2.0` | ➖ | No updater |
| T-15 | Installers code-signed per platform | `manual:signtool/codesign on release artifact` | ⬜ | No signed installer available in this run; not verifiable from the tree |
| T-16 | Headers supply nosniff and the supported security set | `static:headers` | ✅ | `nosniff`, CORP/COOP `same-origin`, device-denying `Permissions-Policy` (F-06, `bcce2f5`) |
| T-17 | Opener scope narrower than `opener:default` | `static:capabilities-opener` | ✅ | No `opener:*`; Cargo and npm plugin removed (F-02, `bcce2f5`) |
| 8.2.1 | Function-level access restricted to explicit permission | `static:capabilities` + `static:command-scope` | ✅ | App commands declared and granted one by one; a new command fails closed (F-04, F-05) |
| 13.1.1 | All communication needs documented, including destinations | `manual:document review (docs + bundle host list)` | ✅ | `security-level.md` §1 egress allowlist; `static:bundle-external-hosts` confirms only `w3.org`/`vuejs.org` literals (F-08, `9732f62`) |
| 13.2.2 | Local interfaces reachable only with least privilege | `static:capabilities` | ✅ | Least-privilege core set (F-04) |
| 13.2.4 | Allowlist defines external resources the app may contact | `static:csp` + `static:bundle-external-hosts` | ✅ | IPC-only `connect-src`; `img-src 'self' data:`; no external host in the bundle (F-01, F-09) |
| 13.3.1 | Secrets are not in source or build artifacts | `static:bundle-secrets` + `static:dead-logo-path` | ✅ | Token and `logo.dev` absent; dead source removed; no `.env` tracked (F-26, `303b778`) |
| 13.3.2 | Access to secret assets follows least privilege | `static:bundle-secrets` | ✅ | No secret inlined; no `.env` in the tree |

---

## 2. Renderer injection and secret exposure

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 1.2.1 | Output encoding matches the context written into | `static:source-injection-sinks` | ✅ | 0 hits for `v-html`/`innerHTML`/`outerHTML`/`document.write`/`insertAdjacentHTML`/`srcdoc`/`DOMParser`; text goes through Vue interpolation |
| 1.2.2 | Dynamically built URLs are encoded and protocol-restricted | `static:source-url-attrs` | ✅ | Every `:src` is an `<img>`; icon values are validated on write and withheld on read (`icons.rs` tests), and CSP `img-src 'self' data:` blocks remote fetch (F-09, `1b0b571`) |
| 1.2.3 | Dynamically built JS/JSON is injection-safe | `static:source-injection-sinks` | ✅ | No runtime template compilation; 0 hits for `new Function`/`eval(`/`compile(` |
| 1.3.2 | No `eval` or dynamic code execution | `static:source-eval` | ✅ | 0 hits in `src/` |
| 1.3.3 | Dangerous-context data is sanitised and length-limited | `rust:command-bounds` | ✅ | `validation.rs` bounds names, note title/body, account/card/identity fields, tags, colours and secrets; `test_item_fields_are_bounded`, `test_vault_metadata_is_bounded`, `test_profile_fields_are_bounded` (F-17, `fbb309b`) |
| 1.3.4 | User-supplied SVG validated/sanitised | `manual:source review (imageUpload.ts)` | ✅ | File-type allowlist, then `Image` decode → 128×128 canvas → PNG `data:` URL (`uploads.md` §2). No renderer test is implemented; this is code review |
| 1.5.2 | Deserialisation uses an allowlist of types | `rust:serde-targets` | ✅ | Backend deserialises into concrete structs; malformed vault JSON is rejected (`test_corrupt_vault_fails_unlock_without_modification`, `models` tests) |
| 3.2.1 | Controls prevent rendering in the wrong context | `static:source-injection-sinks` | ✅ | Same evidence as 1.2.1 |
| 3.2.2 | Text rendered with text APIs, not HTML sinks | `static:source-injection-sinks` | ✅ | Same evidence as 1.2.1 |
| 3.4.3 | CSP sets `object-src`, `base-uri`, and an allowlist | `static:csp` | ✅ | `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`, explicit source allowlists (F-01) |
| 3.4.4 | `X-Content-Type-Options: nosniff` on responses | `static:headers` | ✅ | Header set (F-06) |
| 3.4.5 | Referrer policy limits path/query leakage | `manual:not settable via app.security.headers in Tauri 2.11` | ⚠️ | No header is possible. Residual referrer egress is closed by `connect-src`/`img-src` and opener removal (`tauri-config-baseline.md` §3); this is a mitigated absence, not a configured control |
| 3.5.5 | `postMessage` dropped unless origin trusted and syntax valid | `static:source-postmessage` | ✅ | 0 listeners in `src/`; IPC is `invoke` only |
| 3.7.2 | Automatic redirects only to allowlisted destinations | `static:source-navigation` | ✅ | 0 `href`/`action`/`formaction`/`window.open`; router targets are constants or encoded params |
| 14.2.1 | Sensitive data never placed in a URL or query | `static:source-url-secrets` | ✅ | 0 secret tokens in `src/router`, `useSearch`, `useRouterHistory` |
| 14.2.3 | Defined sensitive data not sent to untrusted parties | `static:csp` + `static:bundle-external-hosts` | ✅ | Remote icon fetch is blocked by CSP and rejected by `icons::validate_icon`/`safe_icon`; egress is IPC-only (F-09) |
| 14.3.1 | Authenticated data cleared when the session ends | `renderer:reveal-timeout` | ✅ | `useSecretReveal` drops the value on 10 s/blur/visibility/unmount (`tests/compliance/secret-reveal.test.ts`); session timeout tears the session down (F-10, F-19) |
| 14.3.2 | Anti-caching headers keep sensitive data out of caches | `manual:custom protocol serves static assets only; no sensitive HTTP response` | ➖ | Vault data travels over IPC, which is not an HTTP cache participant |
| 14.3.3 | Browser storage holds no sensitive data | `static:source-storage` | ✅ | 0 hits for `localStorage`/`sessionStorage`/`indexedDB` |
| P-03 | Reveal re-masks within 10 s and on blur/visibility | `renderer:reveal-timeout` | ✅ | 10 s timer plus global `blur`/`visibilitychange` hide-all (`tests/compliance/secret-reveal.test.ts`) (F-10, `97a6599`) |
| P-04 | Clipboard compare-and-cleared at 30 s, cleared on logout | `rust:clipboard-expiry` | ✅ | Core writes the secret and compare-clears after 30 s; newer copies keep their own window; `clipboard::clear()` on logout/window close (`test_clipboard_expiry_only_clears_the_latest_copy`) (F-11) |
| P-05 | Never-hold list respected | `static:source-storage` + `static:source-logs` + `static:bundle-dead-code` | ✅ | No store/URL/storage path holds a secret; console calls are labelled and do not name secrets; the `seed.ts` fixture is not bundled (F-11, F-26) |
| P-06 | Masking invariant | `manual:source review (secrets.ts)` | ✅ | Masks expose nothing of the value except the card number's last four digits, by design (R-S6, `src/util/secrets.ts`). No renderer test is implemented |
| P-07 | No false zeroization of JS strings | `static:source-zeroization` | ✅ | 0 scrub attempts; the policy forbids them (R-S7) |

---

## 3. IPC and authorisation

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 2.1.1 | Input-validation rules documented per data item | `manual:document review (validation rules)` | ✅ | `ipc-validation.md` §2–3 lists command categories, field bounds and the ownership rule (F-16, `fbb309b`) |
| 2.2.1 | All input validated against allowlists/patterns/ranges | `rust:command-bounds` | ✅ | UUID parsing, kind/field-key allowlists and per-field character bounds before mutation (F-17) |
| 2.2.2 | Validation enforced at a trusted layer | `rust:command-validation` | ✅ | All checks live in `commands.rs`/`vault_manager.rs`/`validation.rs`, never only in Vue |
| 2.3.2 | Documented business-logic limits implemented | `manual:document review (auth-policy.md §2, ipc-validation.md §3) + rust:login-rate-limit` | ✅ | Login throttle and field bounds are documented and enforced in the core (F-15, F-16, F-17) |
| 2.4.1 | Anti-automation controls on abusable functions | `rust:login-rate-limit` | ✅ | Five failures block for 5 min, doubling to a 15-min cap; persisted across restart (`test_login_backoff_grows_and_caps`, `test_login_throttle_survives_restart`) (F-15, `7e6b85e`) |
| 8.1.1 | Function- and data-authorisation rules documented | `manual:document review (command→caller matrix)` | ✅ | `ipc-validation.md` §1–2 records `vault_ids` as the ACL and the command categories (F-16) |
| 8.2.2 | Data-specific access requires explicit permission (IDOR/BOLA) | `rust:ownership` | ✅ | Every vault/item command calls `require_owned_vault`; scans are scoped by containing vault (`test_require_owned_vault_rejects_an_unowned_vault_id`, `test_get_all_items_excludes_vaults_the_active_user_does_not_own`) (F-12, F-14) |
| 8.2.3 | Field-level read/write restricted (BOPLA) | `rust:secret-surface` | ✅ | `SafeItem`/`SafeVault` omit secrets; `get_item_secret` is the only egress and validates kind + field key (`test_get_item_secret_errors`, `test_secret_input_validation_per_kind`) |
| 8.3.1 | Authorisation enforced at a trusted layer | `rust:ownership` | ✅ | Core checks ownership; renderer-supplied ids are untrusted (F-12) |
| 15.3.1 | Only required fields returned from data objects | `rust:secret-surface` | ✅ | Safe-view pattern; note bodies and identity PII remain by product design (`asset-tiers.md`) |
| 15.3.3 | Mass assignment prevented with per-action allowlists | `rust:update-allowlist` | ✅ | `update_item` takes named fields only; ids, timestamps and `vault_ids` are not caller-settable (`commands.rs:676`) |
| P-01 | Renderer untrusted for authorisation (R1/R2) | `rust:ownership` | ✅ | Every command re-checks ownership and validates arguments in the core (F-12) |
| P-02 | Switching the active User ends every prior session | `rust:profile-switch-lock` | ✅ | `login_user` and `register_user` call `end_session` first; prior vaults locked and credentials zeroized (`test_login_ends_the_previous_session`, `test_register_ends_the_previous_session`) (F-13) |

---

## 4. Authentication and session

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 6.1.1 | Rate limiting and anti-automation documented | `manual:document review (auth policy)` | ✅ | `auth-policy.md` §2 documents the counter, backoff cap and persistence (F-20, `7e6b85e`) |
| 6.2.1 | Minimum password length enforced | `rust:password-policy` | ✅ | 12-character floor at registration (`test_master_password_policy`) (F-18) |
| 6.2.4 | New passwords checked against common passwords | `rust:password-policy` | ✅ | zxcvbn score ≥3/4 at registration (`test_master_password_policy`) (F-18) |
| 6.2.6 | Password inputs use `type=password` | `static:source-password-inputs` | ✅ | `RegisterView.vue`, `LoginView.vue` |
| 6.2.8 | Passwords verified exactly, no truncation/case change | `rust:password-verbatim` | ✅ | The exact string is fed to the KDF; `user_manager.login`/`decrypt_secret_key` do not trim or fold. No dedicated test; code review |
| 6.2.10 | No forced periodic credential rotation | `rust:no-rotation` | ✅ | `User` has no expiry or rotation field; no prompt exists |
| 6.2.11 | Context-specific word list blocks guessable passwords | `rust:password-policy` | ✅ | zxcvbn inputs include the user's name and `lockr`/`vault`/`password` (`user_manager.rs:22,85-99`) (F-18) |
| 6.2.12 | Breached-password check | `manual:standing deviation (local-first)` | ❌ | Accepted standing deviation: a breach lookup needs a network call (`security-level.md` §4) |
| 6.3.1 | Brute-force/credential-stuffing controls | `rust:login-rate-limit` | ✅ | Persistent backoff, capped at 15 min (F-15) |
| 6.3.3 | MFA, or a combination of single factors | `manual:standing deviation (single factor)` | ❌ | Accepted standing deviation: the secret key is encrypted under the master password and is not independent possession (`security-level.md` §4.1) |
| 7.1.1 | Inactivity and absolute lifetimes documented with justification | `manual:document review (session policy)` | ✅ | `auth-policy.md` §3 records 15 min idle and 12 h absolute, with rationale (F-20) |
| 7.3.1 | Inactivity timeout forces re-authentication | `rust:session-timeout` | ✅ | Core enforces 15 min idle (`test_session_expires_after_the_idle_timeout`) (F-19, `28d9c77`) |
| 7.3.2 | Absolute maximum session lifetime | `rust:session-timeout` | ✅ | Core enforces 12 h absolute (`test_absolute_lifetime_expires_even_while_active`) (F-19) |
| 7.4.1 | A terminated session cannot be used again | `rust:session-termination` | ✅ | `logout` flushes, locks every vault and zeroizes credentials (`test_end_session_locks_vaults_and_clears_credentials`) |
| 7.4.2 | Terminating an account terminates its sessions | `rust:profile-switch-lock` | ✅ | `delete_user` clears the session before removing the profile; `end_session` is shared with logout |

---

## 5. Cryptography, files and data protection

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 5.1.1 | Permitted file types, extensions and max sizes documented | `manual:document review (upload spec)` | ✅ | `uploads.md` §1 records PNG/JPEG/WebP/GIF, 8 MiB input, 128 × 128 PNG output (F-21, `9732f62`) |
| 5.2.2 | Accepted files checked so content matches extension | `manual:source review (imageUpload.ts)` | ✅ | The file is decoded as an image, drawn to canvas and re-encoded PNG; a non-image polyglot fails to decode (`uploads.md` §2). No renderer test is implemented |
| 5.3.2 | File paths built from trusted/internal data | `rust:uuid-traversal` | ✅ | Every id is parsed to `Uuid` before `vault_path()` formats it (`vault_manager.rs:74`); free text never reaches a path |
| 5.4.1 | Download filenames validated or app-generated | `manual:no export/download feature in 1.2.0` | ➖ | Import/export are disabled "coming soon" in Settings |
| 11.1.2 | Cryptographic inventory lists keys, algorithms, uses | `manual:document review (crypto inventory)` | ✅ | `crypto-inventory.md` lists primitives, parameters, key hierarchy, uses and the file format (F-22, `9732f62`) |
| 11.2.1 | Industry-validated crypto implementations | `static:deps-crypto` | ✅ | `argon2` 0.5 and `aes-gcm` 0.11, both maintained RustCrypto crates |
| 11.2.2 | Crypto agility: algorithms/keys can be swapped | `rust:format-version` | ⚠️ | `schema_version` enables migration, but the file format carries no algorithm identifier; a cipher change is a schema migration, not a per-file tag (`crypto-inventory.md` §3) |
| 11.3.2 | Approved ciphers and modes (AEAD) | `rust:crypto-attrs` | ✅ | AES-256-GCM, fresh 12-byte nonce per encryption (`crypto.rs:91,94`; `test_fresh_nonce_each_encryption`) |
| 11.4.1 | Approved hash functions | `rust:crypto-attrs` | ✅ | No MD5/SHA-1 in `src-tauri`; Argon2id is the only KDF |
| 11.4.2 | Passwords stored with a computationally intensive KDF | `rust:crypto-attrs` | ✅ | Argon2id v0x13, m=64 MiB, t=3, p=4 — above the OWASP minimum (`crypto.rs:43-51`) |
| 11.5.1 | Non-guessable values from a CSPRNG with ≥128 bits | `rust:crypto-attrs` | ✅ | `OsRng` for the 32-byte salt, 12-byte nonce and 32-byte secret key (`crypto.rs:26-29,94-95`) |
| 12.2.1 | TLS for all external HTTP, no insecure fallback | `static:bundle-external-hosts` | ✅ | The shipped app makes no external HTTP call; the only host literals are `w3.org` and `vuejs.org`, neither fetched |
| 12.3.2 | TLS clients validate certificates | `manual:no external TLS calls in 1.2.0` | ➖ | Nothing to validate; revisit with any updater or network feature |
| 14.1.1 | Sensitive data identified and classified | `manual:document review (threat-model.md §2)` | ✅ | A1–A6 in three tiers |
| 14.1.2 | Each protection level has documented requirements | `manual:document review (asset-tiers.md)` | ✅ | Per-tier at-rest, safe-view, masking, clipboard, logging, retention and network rules (F-23, `9732f62`) |

**Scope note:** these rows assert configuration and usage facts (primitives, parameters, formats).
They are not cryptanalysis; correctness of the vault format is ADR-0001's authority (threat model
AS6).

---

## 6. Supply chain and release hygiene

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 13.4.1 | No VCS metadata shipped in the deployment | `manual:inspect installer payload` | ⬜ | `dist/` contains no `.git`/`.svn`, but the installer payload was not available in this run; not verifiable from the tree |
| 13.4.2 | Debug modes disabled in production | `manual:release build, attempt inspector` | ⬜ | Source checks hold (`static:devtools-absent`; sourcemaps follow `TAURI_ENV_DEBUG`), but no release artefact was available to confirm |
| 15.1.2 | SBOM/inventory of third-party libraries maintained | `static:deps-inventory` | ✅ | `docs/security/sbom.json` (CycloneDX 1.5) is committed and regenerated by `yarn sbom`; the stale check passes (F-24, `2139ad7`) |
| 15.2.1 | No component past its remediation timeframe | `static:deps-audit` | ✅ | `yarn npm audit --severity high` is clean; `dependency-policy.md` records the remediation window (F-25, `303b778`). The Rust advisory gate remains manual |
| 15.2.3 | Production contains only required functionality | `static:bundle-dead-code` | ✅ | `dist/` has no seed fixture or dev-only module, no source map and no token |
| 15.3.6 | JavaScript resists prototype pollution | `static:freeze-prototype` | ✅ | `freezePrototype: true` (F-03) |
| P-08 | Disabled-feature code and tokens absent from the bundle | `static:bundle-secrets` + `static:dead-logo-path` | ✅ | 0 hits for `logo.dev`, `VITE_LOGO_API_TOKEN`, `Dev seed data`, `test12`; the dead source was removed (F-26) |
| P-09 | No network egress from the renderer | `static:csp` + `static:bundle-external-hosts` | ✅ | IPC-only `connect-src`, `img-src 'self' data:`, validated icons; no external host literal beyond `w3.org`/`vuejs.org` (F-01, F-09) |

---

## 7. Logging and error handling

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 16.1.1 | A log inventory covers events, format, storage, access, retention | `manual:document review (logging policy)` | ✅ | `logging-policy.md` §1–4 records the auth-log events, format, location, reader and retention (F-27, `9732f62`) |
| 16.2.5 | Logging of sensitive data follows its protection level | `static:source-logs` | ✅ | No secret is written to stdout, stderr or the console; `auth.log` carries identifiers and timestamps only |
| 16.3.1 | Authentication success and failure are logged | `rust:auth-logging` | ✅ | `registered`, `login_success`, `login_failed`, `login_blocked`, `reauth_*` (`test_auth_log_records_events_without_secrets`) (F-15, F-27) |
| 16.3.2 | Failed authorisation attempts are logged | `rust:auth-logging` + `manual:logging-policy.md §2` | ❌ | Authentication failures are logged, but ownership/authorisation denials are deliberately not logged (`logging-policy.md` §2). Accepted omission; the row is not met as written |
| 16.3.4 | Unexpected errors and control failures are logged | `manual:document review (logging policy)` | ❌ | Unexpected I/O and serialization errors are not logged; the UI gets a generic message only (`logging-policy.md` §2). Accepted omission; the row is not met as written |
| 16.5.1 | Generic error messages, no stack traces or secrets | `rust:error-messages` | ✅ | I/O, serialization and not-found errors Display a generic message (`test_io_display_does_not_leak_the_path`, `test_not_found_display_hides_the_identifier`) (F-29) |
| 16.5.3 | Application fails closed on exceptions | `rust:atomic-write` | ✅ | Temp file + `sync_all` + rename (`vault_manager.rs:349`); `test_failed_save_leaves_the_existing_vault_intact`, `test_failed_rename_keeps_the_existing_vault_and_cleans_up`, `test_save_leaves_no_temp_file_and_persists_the_new_content` (F-28, `da46178`) |

---

## Appendix — ASVS chapter → group

| Chapter | Group |
|---|---|
| V1 Encoding and Sanitization | 2 |
| V2 Validation and Business Logic | 3 |
| V3 Web Frontend Security | 2 |
| V5 File Handling | 5 |
| V6 Authentication | 4 |
| V7 Session Management | 4 |
| V8 Authorization | 3 |
| V11 Cryptography | 5 |
| V12 Secure Communication | 5 |
| V13 Configuration | 1, 6 |
| V14 Data Protection | 2, 5 |
| V15 Secure Coding and Architecture | 3, 6 |
| V16 Security Logging and Error Handling | 7 |
| Tauri controls T-01…T-17 | 1 (T-09/T-10/T-11 also 6) |
| Project rows P-01…P-09 | 1–6 as marked |

## Change log

| Date | Commit | Change |
|---|---|---|
| 2026-11-27 | `<sha>` | Initial instrument: 79 ASVS rows, 17 Tauri controls, 9 project rows |
| 2026-11-27 | `f3d5c82` | Initial conformance sweep: 40 met, 17 partial, 41 deviated, 6 n/a, 1 unchecked |
| 2026-11-28 | `a41cde1` | Re-run against the remediated tree: 89 met, 2 partial, 4 deviated, 6 n/a, 4 unchecked; re-pinned |
