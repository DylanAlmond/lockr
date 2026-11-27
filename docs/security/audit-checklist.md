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
2. Run the automated checks first (`scripts/compliance/*`, `cargo test`, `yarn test` once the harness
   from [#74](https://github.com/DylanAlmond/lockr/issues/74) exists). Fill status and evidence for
   every row with a `test-id`.
3. Walk the `manual:` rows in group order, recording what was inspected.
4. A row is never left blank. `n/a` requires a reason in Notes.
5. Findings go to `findings.md` with the citation format in `threat-model.md` §11; the checklist row
   links to the finding id.

**Status vocabulary:** `unchecked` · `met` · `partial` · `deviated` · `n/a` · `fixed`.
Definitions in `evidence-standard.md` §3.

**Verification ids:** `static:<slug>` (scripts over config, source, or built artefact) ·
`rust:<slug>` (`src-tauri/tests/`) · `renderer:<slug>` (vitest) · `manual:<reason>`.
The rule: every row has one, and `manual:` states why automation is not possible.

## 0. Run header

| Field | Value |
|---|---|
| Commit | `f3d5c82` app code (groups 1–2) |
| Date | 2026-11-27 |
| Runner | OpenCode session |
| Toolchain | Node 24.19 · Rust stable · Tauri 2.11.4 |
| Automated checks | not run — harness pending [#74](https://github.com/DylanAlmond/lockr/issues/74) |
| Rows: met / partial / deviated / n/a | group 1: 5 / 7 / 7 / 3 (1 unchecked) · group 2: 14 / 3 / 6 / 1 |

---

## 1. Configuration, capabilities and build settings

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| T-01 | `app.security.csp` is set; no `unsafe-eval`/`unsafe-inline` for scripts | `static:csp` | deviated | F-01 — `csp: null` |
| T-02 | `devCsp` set separately; dev relaxations never ship | `static:csp-dev` | deviated | F-01 — unset, so dev inherits the (absent) production policy |
| T-03 | `dangerousDisableAssetCspModification` absent/false, so Tauri injects nonces | `static:csp-injection` | met | unset → default `false` |
| T-04 | Capabilities are least privilege; no cross-window merge | `static:capabilities` | partial | F-04 — `core:default` + `core:window:default` over-grant |
| T-05 | Commands are scoped per window/capability, not all-to-all | `static:command-scope` | deviated | F-05 — plain `tauri_build::build()` |
| T-06 | IPC args deserialise into concrete types and are validated | `rust:command-validation` | partial | Typed via serde; bounds not enforced (F-01 group 3 will confirm authorisation) |
| T-07 | Isolation pattern used, or its absence recorded as a decision | `manual:declined in tauri-config-baseline.md §6` | met | Declined, recorded |
| T-08 | `freezePrototype` is `true` | `static:freeze-prototype` | deviated | F-03 — unset → default `false` |
| T-09 | Devtools unavailable in release builds | `manual:launch release build, attempt inspector` | partial | No `devtools` Cargo feature; release artefact not inspected |
| T-10 | No source maps in the release bundle | `static:bundle-sourcemaps` | partial | F-07 — absent by Vite default, not asserted |
| T-11 | `withGlobalTauri` is `false` | `static:config-global-tauri` | met | unset → default `false` |
| T-12 | Single-instance plugin first, `argv`/`cwd` treated as untrusted | `manual:not present in 1.2.0` | n/a | No single-instance plugin; row stays for a future addition |
| T-13 | Deep links statically configured; URL shape validated | `manual:not present in 1.2.0` | n/a | No deep-link plugin |
| T-14 | Updater artifacts signed, key pinned, HTTPS endpoints | `manual:not present in 1.2.0` | n/a | No updater |
| T-15 | Installers code-signed per platform | `manual:signtool/codesign on release artifact` | unchecked | No release artefact available in this run |
| T-16 | Headers supply nosniff and the supported security set | `static:headers` | deviated | F-06 — `headers` unset |
| T-17 | Opener scope narrower than `opener:default` | `static:capabilities-opener` | deviated | F-02 — `opener:default` granted, unused |
| 8.2.1 | Function-level access restricted to explicit permission | `static:capabilities` | partial | F-04 |
| 13.1.1 | All communication needs documented, including destinations | `manual:document review (docs + bundle host list)` | partial | F-08 — no written inventory; bundle hosts are `w3.org` and `vuejs.org` |
| 13.2.2 | Local interfaces reachable only with least privilege | `static:capabilities` | partial | F-04 |
| 13.2.4 | Allowlist defines external resources the app may contact | `static:csp` + `static:bundle-external-hosts` | deviated | F-01 — no CSP source list; no egress allowlist |
| 13.3.1 | Secrets are not in source or build artifacts | `static:bundle-secrets` | met | Token and logo.dev absent from `dist/` |
| 13.3.2 | Access to secret assets follows least privilege | `static:bundle-secrets` | met | `.env` untracked; token not inlined |

---

## 2. Renderer injection and secret exposure

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 1.2.1 | Output encoding matches the context written into | `static:source-injection-sinks` | met | 0 hits for `v-html`/`innerHTML`/`outerHTML`/`document.write`/`insertAdjacentHTML`/`srcdoc`/`DOMParser`; text goes through Vue interpolation |
| 1.2.2 | Dynamically built URLs are encoded and protocol-restricted | `static:source-url-attrs` | partial | F-09 — nine `<img :src>` bindings accept a backend string with no scheme allowlist |
| 1.2.3 | Dynamically built JS/JSON is injection-safe | `static:source-injection-sinks` | met | No runtime template compilation; 0 hits for `new Function`/`eval(`/`compile(` |
| 1.3.2 | No `eval` or dynamic code execution | `static:source-eval` | met | 0 hits in `src/` |
| 1.3.3 | Dangerous-context data is sanitised and length-limited | `rust:command-bounds` | partial | No dangerous context exists; string length is unbounded on IPC (assessed in group 3) |
| 1.3.4 | User-supplied SVG validated/sanitised | `renderer:upload-rasterisation` | met | Uploads decode via `Image`, draw to canvas, re-encode PNG/WebP; stored as an image `data:` URL |
| 1.5.2 | Deserialisation uses an allowlist of types | `rust:serde-targets` | met | Backend deserialises into concrete structs; renderer `invoke<T>` is an erased assertion but receives JSON, not code |
| 3.2.1 | Controls prevent rendering in the wrong context | `static:source-injection-sinks` | met | Same evidence as 1.2.1 |
| 3.2.2 | Text rendered with text APIs, not HTML sinks | `static:source-injection-sinks` | met | Same evidence as 1.2.1 |
| 3.4.3 | CSP sets `object-src`, `base-uri`, and an allowlist | `static:csp` | deviated | F-01 — `csp: null` |
| 3.4.4 | `X-Content-Type-Options: nosniff` on responses | `static:headers` | deviated | F-06 — `headers` unset |
| 3.4.5 | Referrer policy limits path/query leakage | `manual:not settable via app.security.headers in Tauri 2.11` | partial | No header possible; residual leakage is the app origin only, and F-09 is the one outbound path |
| 3.5.5 | `postMessage` dropped unless origin trusted and syntax valid | `static:source-postmessage` | met | 0 listeners in `src/`; IPC is `invoke` only |
| 3.7.2 | Automatic redirects only to allowlisted destinations | `static:source-navigation` | met | 0 `href`/`action`/`formaction`/`window.open`; router targets are constants or encoded params |
| 14.2.1 | Sensitive data never placed in a URL or query | `static:source-url-secrets` | met | 0 secret tokens in `src/router`, `useSearch`, `useRouterHistory` |
| 14.2.3 | Defined sensitive data not sent to untrusted parties | `static:csp` + `static:bundle-external-hosts` | deviated | F-09 — a vault-derived icon URL can reach an arbitrary host |
| 14.3.1 | Authenticated data cleared when the session ends | `renderer:secret-lifetime` | deviated | F-10 — revealed secrets persist until item change |
| 14.3.2 | Anti-caching headers keep sensitive data out of caches | `manual:custom protocol serves static assets only; no sensitive HTTP response` | n/a | Vault data travels over IPC, which is not an HTTP cache participant |
| 14.3.3 | Browser storage holds no sensitive data | `static:source-storage` + `renderer:never-hold` | met | 0 hits for `localStorage`/`sessionStorage`/`indexedDB` |
| P-03 | Reveal re-masks within 10 s and on blur/visibility | `renderer:reveal-timeout` | deviated | F-10 — no timer; the 10 `@blur` handlers are field-level, not window-level |
| P-04 | Clipboard compare-and-cleared at 30 s, cleared on logout | `renderer:clipboard-expiry` | deviated | F-11 — one `writeText`, no clear anywhere |
| P-05 | Never-hold list respected | `renderer:never-hold` | met | No store/URL/storage path holds a secret; console calls log history and labels, not values (the `seed.ts` fixture logs a test password but is not bundled) |
| P-06 | Masking invariant | `renderer:masking` | met | Masks expose nothing of the value except the card number's last four digits, by design (R-S6) |
| P-07 | No false zeroization of JS strings | `static:source-zeroization` | met | 0 scrub attempts; the policy forbids them (R-S7) |

---

## 3. IPC and authorisation

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 2.1.1 | Input-validation rules documented per data item | `manual:document review (validation rules)` | unchecked | |
| 2.2.1 | All input validated against allowlists/patterns/ranges | `rust:command-validation` | unchecked | |
| 2.2.2 | Validation enforced at a trusted layer | `rust:command-validation` | unchecked | |
| 2.3.2 | Documented business-logic limits implemented | `rust:login-limits` | unchecked | |
| 2.4.1 | Anti-automation controls on abusable functions | `rust:login-rate-limit` | unchecked | known open: no attempt limiting |
| 8.1.1 | Function- and data-authorisation rules documented | `manual:document review (command→caller matrix)` | unchecked | |
| 8.2.2 | Data-specific access requires explicit permission (IDOR/BOLA) | `rust:ownership` | unchecked | known open: no `vault_ids` check |
| 8.2.3 | Field-level read/write restricted (BOPLA) | `rust:secret-surface` | unchecked | |
| 8.3.1 | Authorisation enforced at a trusted layer | `rust:ownership` | unchecked | known open |
| 15.3.1 | Only required fields returned from data objects | `rust:secret-surface` | unchecked | |
| 15.3.3 | Mass assignment prevented with per-action allowlists | `rust:update-allowlist` | unchecked | |
| P-01 | Renderer untrusted for authorisation (R1/R2) | `rust:ownership` | unchecked | |
| P-02 | Switching the active User ends every prior session | `rust:profile-switch-lock` | unchecked | known open |

---

## 4. Authentication and session

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 6.1.1 | Rate limiting and anti-automation documented | `manual:document review (auth policy)` | unchecked | |
| 6.2.1 | Minimum password length enforced | `rust:password-policy` | unchecked | known open |
| 6.2.4 | New passwords checked against common passwords | `rust:password-policy` | unchecked | known open: zxcvbn advisory |
| 6.2.6 | Password inputs use `type=password` | `static:source-password-inputs` | unchecked | |
| 6.2.8 | Passwords verified exactly, no truncation/case change | `rust:password-verbatim` | unchecked | |
| 6.2.10 | No forced periodic credential rotation | `rust:no-rotation` | unchecked | |
| 6.2.11 | Context-specific word list blocks guessable passwords | `rust:password-policy` | unchecked | |
| 6.2.12 | Breached-password check | `manual:standing deviation (local-first)` | unchecked | record the trade-off in security-level §4 |
| 6.3.1 | Brute-force/credential-stuffing controls | `rust:login-rate-limit` | unchecked | known open |
| 6.3.3 | MFA, or a combination of single factors | `manual:standing deviation (single factor)` | unchecked | security-level §4.1 |
| 7.1.1 | Inactivity and absolute lifetimes documented with justification | `manual:document review (session policy)` | unchecked | |
| 7.3.1 | Inactivity timeout forces re-authentication | `renderer:idle-lock` | unchecked | known open: no idle lock |
| 7.3.2 | Absolute maximum session lifetime | `renderer:idle-lock` | unchecked | |
| 7.4.1 | A terminated session cannot be used again | `rust:session-termination` | unchecked | |
| 7.4.2 | Terminating an account terminates its sessions | `rust:profile-switch-lock` | unchecked | known open |

---

## 5. Cryptography, files and data protection

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 5.1.1 | Permitted file types, extensions and max sizes documented | `manual:document review (upload spec)` | unchecked | |
| 5.2.2 | Accepted files checked so content matches extension | `renderer:upload-rasterisation` | unchecked | |
| 5.3.2 | File paths built from trusted/internal data | `rust:uuid-traversal` | unchecked | |
| 5.4.1 | Download filenames validated or app-generated | `manual:no export/download feature in 1.2.0` | unchecked | n/a if still absent |
| 11.1.2 | Cryptographic inventory lists keys, algorithms, uses | `manual:document review (crypto inventory)` | unchecked | |
| 11.2.1 | Industry-validated crypto implementations | `static:deps-crypto` | unchecked | |
| 11.2.2 | Crypto agility: algorithms/keys can be swapped | `rust:format-version` | unchecked | |
| 11.3.2 | Approved ciphers and modes (AEAD) | `rust:crypto-attrs` | unchecked | |
| 11.4.1 | Approved hash functions | `rust:crypto-attrs` | unchecked | |
| 11.4.2 | Passwords stored with a computationally intensive KDF | `rust:crypto-attrs` | unchecked | |
| 11.5.1 | Non-guessable values from a CSPRNG with ≥128 bits | `rust:crypto-attrs` | unchecked | |
| 12.2.1 | TLS for all external HTTP, no insecure fallback | `static:bundle-external-hosts` | unchecked | |
| 12.3.2 | TLS clients validate certificates | `manual:no external TLS calls in 1.2.0` | unchecked | n/a if still true |
| 14.1.1 | Sensitive data identified and classified | `manual:document review (threat-model.md §2)` | unchecked | |
| 14.1.2 | Each protection level has documented requirements | `manual:document review (threat-model.md §2)` | unchecked | |

**Scope note:** these rows assert configuration and usage facts (primitives, parameters, formats).
They are not cryptanalysis; correctness of the vault format is ADR-0001's authority (threat model
AS6).

---

## 6. Supply chain and release hygiene

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 13.4.1 | No VCS metadata shipped in the deployment | `manual:inspect installer payload` | unchecked | |
| 13.4.2 | Debug modes disabled in production | `manual:release build, attempt inspector` | unchecked | |
| 15.1.2 | SBOM/inventory of third-party libraries maintained | `manual:no SBOM tooling in 1.2.0` | unchecked | |
| 15.2.1 | No component past its remediation timeframe | `static:deps-audit` | unchecked | known open: 5 Vite advisories |
| 15.2.3 | Production contains only required functionality | `static:bundle-dead-code` | unchecked | |
| 15.3.6 | JavaScript resists prototype pollution | `static:freeze-prototype` | unchecked | |
| P-08 | Disabled-feature code and tokens absent from the bundle | `static:bundle-secrets` | unchecked | |
| P-09 | No network egress from the renderer | `static:csp` + `static:bundle-external-hosts` | unchecked | |

---

## 7. Logging and error handling

| ID | Requirement | Verification | Status | Notes |
|---|---|---|---|---|
| 16.1.1 | A log inventory covers events, format, storage, access, retention | `manual:document review (logging policy)` | unchecked | |
| 16.2.5 | Logging of sensitive data follows its protection level | `static:source-logs` | unchecked | |
| 16.3.1 | Authentication success and failure are logged | `rust:auth-logging` | unchecked | known open: no log sink |
| 16.3.2 | Failed authorisation attempts are logged | `rust:auth-logging` | unchecked | known open |
| 16.3.4 | Unexpected errors and control failures are logged | `rust:auth-logging` | unchecked | |
| 16.5.1 | Generic error messages, no stack traces or secrets | `rust:error-messages` | unchecked | |
| 16.5.3 | Application fails closed on exceptions | `rust:atomic-write` + `manual:failure injection` | unchecked | |

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
