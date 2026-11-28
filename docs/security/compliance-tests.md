# Compliance test suite

Specification for the automated half of the audit checklist. Decision record for
[#74](https://github.com/DylanAlmond/lockr/issues/74) (parent
[#63](https://github.com/DylanAlmond/lockr/issues/63)). Nothing here is implemented; this is the plan
that turns `audit-checklist.md` rows into runnable checks.

The rule it serves, from `evidence-standard.md` §4.3: **every checklist row names a test id or a
manual reason.** This document defines what each id actually runs.

## 1. Harness decision

| Layer | Harness | Why |
|---|---|---|
| Static (config, source, built artefact) | Plain Node scripts, no dependencies | Runs anywhere, no install step, easy to read in review |
| Rust | `cargo test` integration tests under `src-tauri/tests/` | The existing 45 unit tests already live in-tree; integration tests get the public API |
| Renderer | **Add vitest + `@vue/test-utils` + jsdom as devDependencies** | Component behaviour (timers, blur, clipboard) cannot be checked statically, and the alternative is permanent manual rows |

The renderer decision is the only dependency change, and it is deliberate: F-10 and F-11 are
behavioural, so without a DOM test runner they stay manual forever.

Additions to `package.json`:

```json
"scripts": {
  "test": "vitest run",
  "test:compliance": "node scripts/compliance/run.mjs"
}
```

## 2. Id convention

| Prefix | Lives in | Run with |
|---|---|---|
| `static:<slug>` | `scripts/compliance/<slug>.mjs`, aggregated by `run.mjs` | `yarn test:compliance` |
| `rust:<slug>` | `src-tauri/tests/compliance_<area>.rs` | `cargo test` |
| `renderer:<slug>` | `tests/compliance/<slug>.test.ts` | `yarn test` |
| `manual:<reason>` | nowhere — a person inspects and records evidence | by hand |

A test must print the checklist id it satisfies on failure, so a red run maps straight back to a row.

## 3. Static suite

One module per area, each exporting `{ id, run }`. `run.mjs` executes them all, prints every result,
and exits non-zero if any check failed.

**Implemented in `bcce2f5`:** `csp.mjs`, `capabilities.mjs` and `run.mjs`, wired as
`yarn test:compliance`. `static:command-scope` is deferred until F-05; `source.mjs`, `bundle.mjs` and
`deps.mjs` are still pending.

### `csp.mjs`

| Id | Assertion |
|---|---|
| `static:csp` | `app.security.csp` is not null; contains `default-src 'self'`, `object-src 'none'`, `base-uri 'none'`, `frame-ancestors 'none'`, `form-action 'none'`; `script-src` has no `'unsafe-inline'`/`'unsafe-eval'`; `connect-src` equals `ipc: http://ipc.localhost` |
| `static:csp-dev` | `devCsp` is set; its `script-src` is no weaker than production's |
| `static:csp-injection` | `dangerousDisableAssetCspModification` is absent or `false` |
| `static:headers` | `headers` contains `X-Content-Type-Options: nosniff` |
| `static:freeze-prototype` | `freezePrototype === true` |
| `static:config-global-tauri` | `withGlobalTauri !== true` |

### `capabilities.mjs`

| Id | Assertion |
|---|---|
| `static:capabilities` | Permission list equals the expected eight-entry set (config baseline §5) |
| `static:capabilities-opener` | No permission starts with `opener:`; `tauri-plugin-opener` absent from `Cargo.toml` and `package.json` |
| `static:command-scope` | `build.rs` references `AppManifest::commands` (currently expected to fail) |

### `source.mjs`

| Id | Assertion |
|---|---|
| `static:source-injection-sinks` | Zero matches in `src/` for `v-html`, `innerHTML`, `outerHTML`, `document.write`, `insertAdjacentHTML`, `srcdoc`, `DOMParser`, `new Function`, `eval(`, `compile(` |
| `static:source-eval` | Subset of the above, reported separately for 1.3.2 |
| `static:source-postmessage` | Zero `postMessage` and zero `addEventListener('message'` |
| `static:source-storage` | Zero `localStorage`, `sessionStorage`, `indexedDB` |
| `static:source-navigation` | Zero `window.open`, `target="_blank"`, literal `<a `, `:href`, `:action`, `formaction` |
| `static:source-url-attrs` | Every `:src` binding is an `<img>`; no `:href` |
| `static:source-url-secrets` | No `password`/`secret`/`card_number` token in `src/router`, `useSearch.ts`, `useRouterHistory.ts` |
| `static:source-zeroization` | Zero `.fill(0)`, `zeroize(`, or overwrite-then-drop patterns in `src/` |
| `static:source-logs` | Every `console.*` call site is in the allowlist (dev fixture, error labels); none interpolates a value from a variable named `password`/`secret`/`value` |
| `static:source-password-inputs` | `LoginView` and `RegisterView` pass `type="password"` |
| `static:source-upload-accept` | `input.accept` is set and `file.type` is checked |

### `bundle.mjs` (runs after `vite build`)

| Id | Assertion |
|---|---|
| `static:bundle-sourcemaps` | Zero `*.map` files; zero `sourceMappingURL` |
| `static:bundle-secrets` | The `.env` token value, `logo.dev`, `VITE_LOGO_API_TOKEN`, `Dev seed data` and `test12` all have zero hits |
| `static:bundle-dead-code` | No dev-only module name appears |
| `static:bundle-external-hosts` | Host set ⊆ `{www.w3.org, vuejs.org}` |

### `deps.mjs`

| Id | Assertion |
|---|---|
| `static:deps-audit` | The dependency audit reports zero high/critical advisories |
| `static:deps-crypto` | Lockfile resolves `argon2` and `aes-gcm` |
| `static:command-validation` | (static half) every `#[tauri::command]` argument is a concrete type, not `serde_json::Value` |

## 4. Rust suite

`src-tauri/tests/compliance_*.rs`. Each test names its checklist id in a comment and asserts one
control.

| Id | Subject | Expected |
|---|---|---|
| `rust:secret-kind` | `get_item_secret` with a key illegal for the kind | rejected |
| `rust:secret-surface` | `serde_json::to_value(SafeItem)` | no secret field names present |
| `rust:ownership` | every vault/item command with a vault not in `active_user.vault_ids` | rejected (after F-12) |
| `rust:profile-switch-lock` | `login_user` for a second User | prior vaults locked (after F-13) |
| `rust:session-termination` | `get_item_secret` after `logout` | `VaultLocked` |
| `rust:uuid-traversal` | ids like `../x`, `..%2f`, non-UUID | rejected before any path use |
| `rust:atomic-write` | save interrupted between write and rename | original file intact (after F-28) |
| `rust:clipboard-expiry` | copied secret at the TTL, clipboard unchanged / replaced | cleared only if unchanged; a newer copy keeps its own window |
| `rust:serde-targets` | malformed vault JSON | rejected with a typed error |
| `rust:crypto-attrs` | Argon2 params and AEAD mode | Argon2id, m=64 MiB, t=3, p=4; AES-256-GCM |
| `rust:format-version` | a saved vault | carries `schema_version` |
| `rust:password-policy` | registration with a short/weak password | rejected (after F-18) |
| `rust:password-verbatim` | leading/trailing spaces | preserved end to end |
| `rust:no-rotation` | `User` shape | no expiry field |
| `rust:login-rate-limit` | repeated failures | backoff after N (after F-15) |
| `rust:auth-logging` | success, failure, denial | one record each (after F-27) |
| `rust:error-messages` | serialized `VaultError::Io` | contains no filesystem path (after F-29) |
| `rust:update-allowlist` | `update_item` with an id/timestamp argument | not accepted |
| `rust:command-bounds` | strings beyond the per-field maximum | rejected (after F-17) |

## 5. Renderer suite

`tests/compliance/*.test.ts`, vitest with jsdom and `@vue/test-utils`. Fake timers for anything
time-based. **Implemented (F-10):** `tests/compliance/secret-reveal.test.ts` covers
`renderer:reveal-timeout`.

Clipboard expiry is `rust:clipboard-expiry`, not a renderer row: the copy path lives in the core
(secret-lifetime §5).

| Id | Subject | Expected |
|---|---|---|
| `renderer:reveal-timeout` | reveal a password, advance 10 s | re-masked; value ref is `null` |
| `renderer:idle-lock` | no interaction for the configured timeout | vault locks (after F-19) |
| `renderer:never-hold` | after a reveal | `localStorage`/`sessionStorage` empty; route has no secret |
| `renderer:masking` | masked card/account fields | no character of the value except the card last four |
| `renderer:upload-rasterisation` | upload an SVG | stored value is a PNG `data:` URL |
| `renderer:icon-scheme` | item with an `https://` icon | no request issued (after CSP) |

## 6. Manual residue

These rows cannot be automated, and each says why. Together they are the "how it was measured" caveat
in `security-level.md`.

| Row | Reason |
|---|---|
| T-15 | Installer signing needs a signed release artefact |
| 13.4.1 | Installer payload contents cannot be inspected without the artefact |
| 13.4.2 / T-09 | Devtools absence is confirmed by launching a release build |
| 3.4.5 | Tauri 2.11 cannot set `Referrer-Policy`; the residual is argued, not tested |
| 14.3.2 | No sensitive HTTP response exists to mark `no-store` |
| 12.3.2 | No external TLS call exists |
| 5.4.1 | No export/download feature exists |
| T-12 / T-13 / T-14 | Feature absent; row is retained for a future addition |
| 2.1.1, 6.1.1, 7.1.1, 8.1.1, 11.1.2, 13.1.1, 14.1.1, 14.1.2, 16.1.1 | Documentation review; a script could assert a file exists, but not that it is correct |

## 7. What is explicitly not included

- **CI wiring**, dependency pinning and SBOM generation — out of scope for this map.
- **Cryptanalysis** — `rust:crypto-attrs` asserts parameters, not correctness.
- **Memory residency** — no test can prove a JS string was erased; `renderer:reveal-timeout` asserts
  the reference is dropped, which is the claim the policy actually makes.
- **OS-level facts** — file permissions, screen capture, clipboard history.

## 8. Order of work

1. `scripts/compliance/` + `run.mjs` — cheapest, catches F-01, F-04–F-08, F-26.
2. Rust integration tests — highest value per line, especially `rust:ownership` and `rust:atomic-write`.
3. vitest harness — needed for F-10, F-11, F-19.
