# Audit baseline: ASVS 5.0 Level 2 (client tier) + Tauri 2 hardening

Research record for [#65](https://github.com/DylanAlmond/lockr/issues/65). This file assembles and
verifies requirements only. It decides nothing, proposes no fixes, and makes no policy call.

Sources pinned at time of writing (retrieved 2026-11-27):

- **ASVS 5.0**, `master` branch, `5.0/en/` — <https://github.com/OWASP/ASVS/tree/master/5.0/en>
- **Tauri 2 docs**, `v2` branch, published at <https://v2.tauri.app/>
- **Tauri config schema 2**, <https://schema.tauri.app/config/2>; cross-checked against
  `tauri` crate v2.11.4 (`crates/tauri-utils/src/config.rs`), which is what `src-tauri/Cargo.lock` resolves.

Baseline evaluated: Lockr 1.2.0, Tauri 2.11.4, Vue 3 SPA, one window labelled `main`, vault data owned by
the Rust core under `app_data_dir`, 30 `#[tauri::command]` functions in `src-tauri/src/commands.rs`.

## 1. Scope rule and baseline facts

"Level 2" throughout means **all L1 plus all L2 requirements**, not L2-only (ASVS 5.0, *What is the ASVS*:
an L2 application implements roughly 70% of the standard, being all L1 and L2 rows). L3-only rows are out.

A requirement is **client tier** if a reviewer can observe it in (a) the webview, (b) the Rust core reachable
over IPC, or (c) the shipped bundle/installer. Requirements whose only enforcement point is an HTTP server,
a cookie jar, a remote IdP, or a service-to-service link are dropped, with reasons in §4.

Applicability was assessed against these observed facts, all read from the tree (no git commands run):

| Fact | Value |
| --- | --- |
| `app.security.csp` | `null` |
| `app.security.devCsp`, `headers`, `pattern`, `freezePrototype` | unset (defaults: no dev CSP, no headers, `brownfield`, `false`) |
| `app.withGlobalTauri` | `false` |
| Capabilities | one file, window `main`, `core:default` + `opener:default` + 4 `core:window:*` grants |
| Command scoping | none; `src-tauri/build.rs` calls plain `tauri_build::build()`, so all 30 commands are callable by `main` |
| `tauri` crate features | `["macos-private-api"]`; no `devtools` |
| Updater / single-instance / deep-link | not present (no plugin dependency, no `plugins` config) |
| Frontend storage | no `localStorage`, `sessionStorage`, or `indexedDB` use |
| Outbound network | one `fetch` to `https://img.logo.dev/...` carrying `VITE_LOGO_API_TOKEN` (`src/util/imageUpload.ts`) |
| Release source maps | none present in `dist/` |

Verdicts are `yes`, `partial`, or `no` for **applicability**, not compliance: `yes` means the requirement
binds Lockr, not that Lockr satisfies it. Evidence of current state lives in the verification column. No
retained row is `no`, because a requirement that does not bind was dropped in §4 rather than given a row.
Requirement text is paraphrased for width; the ID is authoritative and resolves to the cited chapter file.

## 2. Retained ASVS chapters

| Ch. | Title (linked to the requirement file) | Rows | Why it binds a local webview client |
| --- | --- | --- | --- |
| V1 | [Encoding and Sanitization](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x10-V1-Encoding-and-Sanitization.md) | 7 | Vault strings reach the DOM; IPC arguments cross an interpreter boundary |
| V2 | [Validation and Business Logic](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x11-V2-Validation-and-Business-Logic.md) | 5 | Rust commands are the trusted validation layer; unlock is a business flow |
| V3 | [Web Frontend Security](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x12-V3-Web-Frontend-Security.md) | 7 | The webview *is* the frontend; CSP and DOM sinks are the main XSS surface |
| V5 | [File Handling](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x14-V5-File-Handling.md) | 4 | Vault files, icons, and paths are file operations on the user's disk |
| V6 | [Authentication](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x15-V6-Authentication.md) | 10 | The master password is the only gate to plaintext |
| V7 | [Session Management](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x16-V7-Session-Management.md) | 5 | An unlocked vault is a session that must time out and terminate |
| V8 | [Authorization](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x17-V8-Authorization.md) | 5 | Multi-profile support plus a 30-command surface creates function/data/field access questions |
| V11 | [Cryptography](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x20-V11-Cryptography.md) | 7 | Vault confidentiality and password storage are core product claims |
| V12 | [Secure Communication](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x21-V12-Secure-Communication.md) | 2 | logo.dev and any future updater are client-initiated TLS calls |
| V13 | [Configuration](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x22-V13-Configuration.md) | 6 | Build-time secrets, debug modes, and egress allowlists are client configuration |
| V14 | [Data Protection](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x23-V14-Data-Protection.md) | 7 | Plaintext secrets transit and render in the webview |
| V15 | [Secure Coding and Architecture](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x24-V15-Secure-Coding-and-Architecture.md) | 6 | Dependencies, shipped functionality, object shaping, prototype pollution |
| V16 | [Security Logging and Error Handling](https://github.com/OWASP/ASVS/blob/master/5.0/en/0x25-V16-Security-Logging-and-Error-Handling.md) | 7 | Unlock failures and command denials need local records without leaking secrets |

## 3. ASVS 5.0 L1/L2 requirements that bind Lockr

| ID | Requirement (paraphrased) | Lockr | Verification a reviewer can run |
| --- | --- | --- | --- |
| **1.2.1** | Output encoding matches the HTML/CSS/header context it is written into | yes | Grep `v-html` and `innerHTML` in `src/`; confirm vault strings use Vue text interpolation |
| **1.2.2** | Dynamically built URLs are context-encoded and restricted to safe protocols | partial | Review the `encodeURIComponent` build in `src/util/imageUpload.ts`; settles on whether any other URL takes user input |
| **1.2.3** | Dynamically built JS/JSON content is encoded against JS/JSON injection | yes | Grep for `JSON.parse` of input followed by interpolation into a script or template |
| **1.3.2** | No `eval()` or other dynamic code execution | yes | `rg -n "eval\(|new Function" src src-tauri/src` returns nothing |
| **1.3.3** | Data entering a dangerous context is sanitized and length-limited | partial | Enumerate all `invoke` arguments; settles on whether each Rust command bounds string length |
| **1.3.4** | User-supplied SVG is validated or sanitized (no scripts, no `foreignObject`) | partial | Check whether item icons accept SVG uploads; settles on the upload MIME allowlist |
| **1.5.2** | Deserialization of untrusted data uses an allowlist of types; unsafe formats unused | yes | `rg "serde_json::from" src-tauri/src`; confirm every target is a concrete struct |
| **2.1.1** | Input-validation rules are documented per data item | partial | Look for a validation-rules section in `docs/`; absence settles this as unmet, not inapplicable |
| **2.2.1** | All input is validated against allowlists, patterns, ranges, or expected structure | yes | Map each of the 30 command arguments to a typed deserializer or explicit check |
| **2.2.2** | Validation is enforced at a trusted layer, not only client-side | yes | Confirm checks live in `commands.rs`/`models.rs`, not only in Vue form components |
| **2.3.2** | Documented business-logic limits are implemented | partial | Compare documented limits (unlock attempts, item counts) to constants in the Rust core; absence of either settles it as unmet |
| **2.4.1** | Anti-automation controls protect abusable functions | partial | Time repeated failing `login_user` calls; settles on whether a delay or lockout exists |
| **3.2.1** | Controls prevent content being rendered in the wrong context | partial | Inspect custom-protocol responses in devtools Network; settles on whether `Content-Type` is set per asset |
| **3.2.2** | Text is rendered with `textContent`-style APIs, not HTML sinks | yes | `rg "v-html|innerHTML|outerHTML" src` returns nothing |
| **3.4.3** | CSP sets `object-src 'none'`, `base-uri 'none'`, and an allowlist/nonce/hash | yes | `jq -r '.app.security.csp' src-tauri/tauri.conf.json`; `null` means no policy is enforced |
| **3.4.4** | `X-Content-Type-Options: nosniff` on all responses | partial | `jq '.app.security.headers'`; settles on whether Tauri's custom protocol honours configured headers |
| **3.4.5** | A referrer policy limits leakage of path/query data | partial | Same `headers` check; settles on outbound `fetch`/opener navigation behaviour |
| **3.5.5** | `postMessage` messages are dropped unless origin is trusted and syntax valid | partial | `rg "addEventListener\('message'" src`; settles on whether Tauri IPC/isolation paths are the only ones used |
| **3.7.2** | Automatic redirects only go to allowlisted destinations | partial | `rg "openUrl|open\(" src`; settles on the opener capability scope versus an explicit host allowlist |
| **5.1.1** | Permitted file types, extensions, and maximum sizes are documented | partial | Check docs for an upload spec; settles on whether image import is a shipped feature in 1.2.0 |
| **5.2.2** | Accepted files are checked so content matches the claimed extension | partial | Upload HTML bytes named `.png`; settles on whether magic bytes or re-encoding is checked |
| **5.3.2** | File paths are built from trusted/internal data, not user filenames | yes | Inspect path construction in `vault_manager.rs`; confirm UUID-derived paths |
| **5.4.1** | User-submitted download filenames are validated or ignored | partial | If export exists, confirm the filename is app-generated; settles on export feature presence |
| **6.1.1** | Documentation defines rate limiting and anti-automation for brute force | partial | Read the auth section of the docs; settles alongside the 2.4.1 observation |
| **6.2.1** | Passwords are at least 8 characters (15 recommended) | yes | Attempt registration with 7- and 8-character master passwords |
| **6.2.4** | New passwords are checked against at least the top 3000 common passwords | partial | Register with `password`; settles on whether zxcvbn scoring is enforced as a hard gate |
| **6.2.6** | Password input fields use `type=password` | yes | Inspect the master-password input in the rendered DOM |
| **6.2.8** | Passwords are verified exactly as received, with no truncation or case change | yes | Log in with leading/trailing spaces; compare against stored behaviour |
| **6.2.10** | No forced periodic credential rotation | yes | Confirm no expiry field on the user record and no rotation prompt |
| **6.2.11** | A documented context-specific word list blocks guessable passwords | partial | Try the product name and app identifiers as a master password; settles on zxcvbn user-input wiring |
| **6.2.12** | New passwords are checked against a breached-password set | partial | Record current behaviour; settles on whether a network breach check is permitted for a local-first app |
| **6.3.1** | Brute-force and credential-stuffing controls are implemented as documented | partial | Repeat 2.4.1; settles on whether the counter survives an app restart |
| **6.3.3** | MFA, or a combination of single factors, is required to access the application | partial | Enumerate what is required to read an item secret; settles on whether device/OS unlock counts as a factor |
| **7.1.1** | Inactivity timeout and absolute lifetime are documented with justification | partial | Find documented timeouts; settles on whether vault auto-lock constants are written down |
| **7.3.1** | An inactivity timeout forces re-authentication | yes | Idle past the configured timeout; confirm the vault re-locks and key material is dropped |
| **7.3.2** | An absolute maximum session lifetime forces re-authentication | partial | Keep a vault unlocked past a working day; settles on whether lifetime is per-vault or per-process |
| **7.4.1** | A terminated session cannot be used again | yes | After `logout`/lock, invoke `get_item_secret` from the console; it must reject |
| **7.4.2** | Terminating an account also terminates its active sessions | partial | Delete a profile whose vault is unlocked, then invoke a command for that vault; settles on whether profile deletion revokes keys |
| **8.1.1** | Function-level and data-specific authorization rules are documented | partial | Look for a command-to-caller and vault-ownership matrix in `docs/`; absence settles it as unmet |
| **8.2.1** | Function-level access is restricted to consumers with explicit permission | yes | Compare the capability permission list to the 30 registered commands |
| **8.2.2** | Data-specific access requires explicit permission (IDOR/BOLA) | partial | Call `get_vault_by_id` with another profile's UUID; settles on whether ownership is checked in Rust |
| **8.2.3** | Field-level read/write access is restricted (BOPLA) | yes | Confirm `SafeItem` omits secret fields and `get_item_secret` is the only secret path |
| **8.3.1** | Authorization is enforced at a trusted layer, not client-side JavaScript | partial | Confirm vault ownership checks exist in `vault_manager.rs`; settles on multi-profile support being real |
| **11.1.2** | A cryptographic inventory lists keys, algorithms, certificates, and allowed uses | partial | Check for an inventory doc covering Argon2id, AES-256-GCM, salts, nonces, and vault keys; absence settles it as unmet |
| **11.2.1** | Industry-validated implementations are used for cryptographic operations | yes | `cargo tree` shows `argon2` and `aes-gcm`; record versions and advisories |
| **11.2.2** | Crypto agility allows algorithms, keys, and data to be swapped or re-encrypted | partial | Inspect the vault file format for version/algorithm tags; settles on whether rotation is possible |
| **11.3.2** | Only approved ciphers and modes are used, such as AES-GCM | yes | `rg "Aes256Gcm|aes_gcm" src-tauri/src/crypto.rs`; confirm an AEAD mode |
| **11.4.1** | Only approved hash functions are used; MD5 and peers are excluded | yes | `rg -i "md5|sha1" src-tauri/src` returns nothing |
| **11.4.2** | Passwords are stored with a computationally intensive KDF | yes | Read the Argon2 parameters in `crypto.rs` and compare to OWASP guidance |
| **11.5.1** | Non-guessable values come from a CSPRNG with at least 128 bits of entropy | yes | Confirm `OsRng` (or equivalent) is used for salts, keys, and nonces |
| **12.2.1** | TLS is used for all client-to-external HTTP, with no insecure fallback | yes | `rg "http://" src src-tauri`; only the dev server URL should match |
| **12.3.2** | TLS clients validate certificates before communicating | yes | Confirm no certificate-verification bypass is configured for logo.dev or updater calls |
| **13.1.1** | All communication needs are documented, including user-supplied destinations | partial | Check docs for a list of egress hosts; settles on logo.dev being the only one |
| **13.2.4** | An allowlist defines external resources the application may contact | partial | Compare CSP `connect-src`/`img-src` and opener scope to actual egress; a mismatch settles it as unmet |
| **13.3.1** | Secrets are not in source or build artifacts | yes | Run `yarn build`, then search `dist/` for `VITE_LOGO_API_TOKEN` or the token value |
| **13.3.2** | Access to secret assets follows least privilege | partial | Confirm `.env` is ignored and the token is not webview-readable; settles on whether it is inlined at build time |
| **13.4.1** | No source-control metadata is shipped in the deployment | yes | Inspect the installer payload for `.git` or `.svn` |
| **13.4.2** | Debug modes are disabled in production builds | partial | In a release build, attempt Ctrl+Shift+I and right-click Inspect; settles on `devtools` feature state |
| **14.1.1** | Sensitive data is identified and classified into protection levels | partial | Look for a classification list covering master password, item secrets, metadata, and profile names; absence settles it as unmet |
| **14.1.2** | Each protection level has documented protection requirements | partial | Check for encryption, logging, retention, and UI-masking rules per class; absence settles it as unmet |
| **14.2.1** | Sensitive data is never placed in a URL or query string | yes | `rg "password|secret" src/router`; confirm no secret reaches route params |
| **14.2.3** | Defined sensitive data is not sent to untrusted parties | partial | Audit all `fetch` calls; settles on whether only brand names leave the app |
| **14.3.1** | Authenticated data is cleared from client storage when the session ends | partial | Lock the vault, then search `document.body.innerText` for a known secret; settles on in-memory retention |
| **14.3.2** | Anti-caching headers keep sensitive data out of webview caches | partial | `jq '.app.security.headers'`; settles on whether the custom protocol cache is disabled |
| **14.3.3** | Browser storage contains no sensitive data except session tokens | yes | Devtools Application tab after unlock: no vault data in storage |
| **15.1.2** | An SBOM/inventory of third-party libraries is maintained from trusted repositories | partial | Check CI for `cargo cyclonedx` or equivalent output |
| **15.2.1** | No component has breached its remediation timeframe | partial | Run `cargo audit` and `yarn npm audit`; settles on a documented timeframe existing |
| **15.2.3** | Production contains only required functionality, with no test or dev code | partial | Search the release bundle for `seed.ts` fixtures, devtools, and source maps |
| **15.3.1** | Only the required subset of fields is returned from data objects | yes | Inspect `SafeItem`/`SafeVault`; confirm list responses omit secrets |
| **15.3.3** | Mass assignment is prevented with per-action field allowlists | partial | Check whether `update_item` accepts caller-supplied ids, timestamps, or owner fields |
| **15.3.6** | JavaScript resists prototype pollution | partial | Check `freezePrototype`; `rg "Object.assign|__proto__" src`; settles on whether the config flag is enabled |
| **16.1.1** | A log inventory covers events, format, storage, access, and retention | partial | Look for a logging doc; settles on whether any log sink exists at all |
| **16.2.5** | Logging of sensitive data follows the data's protection level | partial | `rg "log::|println!|console.log" src src-tauri/src`; settles on whether secrets appear in any sink |
| **16.3.1** | All authentication operations are logged, success and failure | partial | Perform one successful and one failed unlock; check for entries |
| **16.3.2** | Failed authorization attempts are logged | partial | Invoke a command against a vault the profile does not own; check for a denial record |
| **16.3.4** | Unexpected errors and security-control failures are logged | partial | Force a logo.dev failure; settles on whether the failure is recorded without secrets |
| **16.5.1** | A generic message is returned on error, with no stack traces or secrets | partial | Trigger a Rust error; confirm the UI does not print raw `String` errors containing paths |
| **16.5.3** | The application fails closed and securely on exceptions | partial | Interrupt a save; settles on whether a partial vault file can result |

## 4. Dropped ASVS families, and why

| Dropped | Reason |
| --- | --- |
| **V4 API and Web Service** (all) | Chapter is shaped for HTTP/GraphQL/WebSocket endpoints. Lockr exposes no HTTP API; the equivalent boundary is Tauri IPC, which ASVS does not model (see §6, G1) |
| **V9 Self-contained Tokens**, **V10 OAuth and OIDC** | No JWTs, no federated identity |
| **V17 WebRTC** | No real-time media |
| **V3.3 cookies** (3.3.1–3.3.5), **V3.4.1 HSTS**, **V3.4.2 CORS**, **V3.4.6 frame-ancestors**, **V3.5.1–3.5.3 CSRF/methods**, **V3.5.4 separate hostnames**, **V3.6 SRI**, **V3.1.1** | No cookie jar, no HTTP origin served to a browser, no third-party asset hosting, and the app is not embeddable by a third party. V3.4.3/3.4.4/3.4.5 survive because they can be delivered through `app.security.headers` or the injected CSP. Re-add 3.4.6 if a browser-loadable origin is introduced |
| **V5.2.1, 5.2.3, 5.4.3** | No archive import, no server-side upload path, no antivirus service. 5.2.2/5.3.2 survive for local files |
| **V6.2.2, 6.2.3** | These require a change-password flow, which 1.2.0 does not have. 6.2.5, 6.2.7, 6.2.9 are folded into the 6.2.1 and 6.2.8 rows because the same master-password field is the object of all five checks |
| **V6.4–V6.8** (recovery, MFA mechanisms, OOB, cryptographic auth, IdP) | No recovery service, no TOTP/SMS/push, no external IdP. 6.4.2/6.4.3 would return if account recovery is added |
| **V7.2.x, 7.4.3, 7.4.5, 7.5.1–7.5.2, 7.6.x** | No backend-issued session token, no second session to terminate, no administrator role, no credential-change flow, and no federated sessions. Lock/unlock is the only session mechanism |
| **V8.2.4, V8.3.2–8.3.3**, **V9–V10** | Adaptive/contextual authorization, token-derived decisions, and machine-to-machine delegation do not exist here |
| **V11.6.x, V11.7.x, V12.1.x, V12.3.1, 12.3.3–12.3.5** | No public-key operations, no in-use memory encryption, no server TLS configuration or service mesh. 12.3.2 survives because clients still validate certificates |
| **V13.1.2–13.1.4, 13.2.1, 13.2.3, 13.2.5–13.2.6, 13.3.3–13.3.4, 13.4.3–13.4.7** | Server connection pooling, service accounts, HSMs, web-server directory listing and version banners; 13.2.3 is additionally satisfied by construction (no default accounts) |
| **V14.2.2, 14.2.5–14.2.8** | Server caches, web cache deception, metadata stripping of user uploads |
| **V15.2.2, 15.2.4–15.2.5, 15.3.2, 15.3.4, 15.3.7, 15.4.x** | Availability engineering, dependency-confusion controls, proxy IP forwarding, HTTP parameter pollution, and L3 concurrency rows. The Rust `Mutex<ManagerState>` is the concurrency-relevant artefact but 15.4 is L3 |
| **V16.2.1–16.2.4, 16.4.1, 16.4.3** | Central log pipeline, UTC-synchronised fleet clocks, and off-host log shipping. Local-only logging makes 16.4.3 inapplicable |

**Residual rows inside retained chapters.** Requirements not named above are either L3-only or belong to sink
families this stack does not contain. In V1: 1.2.4–1.2.8 and 1.3.6–1.3.11 cover SQL/ORM, OS command, LDAP,
XPath, LaTeX, SSRF, JNDI, memcache, and mail injection, none of which Lockr reaches; 1.5.1 (XXE) has no XML
parser; 1.3.1 is subsumed by 3.2.2 because no HTML sink exists to sanitize into; 1.4.x is unmanaged-memory
safety, and the Rust core is memory-safe, so the standing check is `rg unsafe src-tauri/src` rather than a row.
In V2: 2.1.2 and 2.1.3 are documentation rows folded into 2.1.1 and 2.3.2; 2.3.3 has no database transaction to
test because vault writes are file flushes; 2.3.4 has no shared finite inventory. The same family-level
reasoning applies to the remaining V8, V13, V14, V15, and V16 rows not listed.

## 5. Tauri 2 hardening controls

These are not ASVS requirements; they are the framework-specific controls that sit under, or beside, the rows above.

| ID | Control | Observed state | Verification | Source |
| --- | --- | --- | --- | --- |
| T-01 | `app.security.csp` is set and does not use `unsafe-eval`/`unsafe-inline` for scripts | absent (`csp` is `null`) | `jq -r '.app.security.csp' src-tauri/tauri.conf.json` returns non-null | [security/csp](https://v2.tauri.app/security/csp/) |
| T-02 | `app.security.devCsp` is set separately, so dev relaxations never ship | absent | `jq -r '.app.security.devCsp' src-tauri/tauri.conf.json` | [config #devcsp](https://v2.tauri.app/reference/config/#devcsp) |
| T-03 | `dangerousDisableAssetCspModification` is absent or `false`, so Tauri injects nonces/hashes for bundled assets | present (default `false`) | `jq '.app.security.dangerousDisableAssetCspModification'`; built `index.html` CSP contains `nonce-` or `sha256-` | [config #dangerousdisableassetcspmodification](https://v2.tauri.app/reference/config/#dangerousdisableassetcspmodification) |
| T-04 | Capabilities grant least privilege, and windows do not share merged capabilities | partial (`core:default` plus `opener:default`) | List each capability's permissions and diff against runtime needs | [security/capabilities](https://v2.tauri.app/security/capabilities/) |
| T-05 | The command surface is scoped per window/capability rather than "all commands to all webviews" | absent | Compare `generate_handler!` count (30) to commands permitted by the capability; check `build.rs` for `AppManifest::commands` | [security/capabilities](https://v2.tauri.app/security/capabilities/) |
| T-06 | IPC arguments are deserialized into concrete types and explicitly validated in Rust | partial (typed via serde; bounds not confirmed) | For each command, confirm a typed argument plus bounds; `serde_json::Value` parameters are the failure signal | [develop/calling-rust](https://v2.tauri.app/develop/calling-rust/) |
| T-07 | The isolation pattern is used, or its absence is a recorded decision | absent (`brownfield`) | `jq '.app.security.pattern'`; settles on whether the frontend dependency set is treated as trusted | [isolation](https://v2.tauri.app/concept/inter-process-communication/isolation/) |
| T-08 | `app.security.freezePrototype` is `true` | absent (default `false`) | `jq '.app.security.freezePrototype'`; also load the app and check `Object.isFrozen(Object.prototype)` | [config #securityconfig](https://v2.tauri.app/reference/config/#securityconfig) |
| T-09 | Devtools are unavailable in release builds | present (no `devtools` feature) | `rg devtools src-tauri/Cargo.toml`; in a release build, Ctrl+Shift+I must not open an inspector | [develop/debug](https://v2.tauri.app/develop/debug/) |
| T-10 | No source maps are shipped in the release bundle | present | `Get-ChildItem dist -Recurse -Filter *.map`; check `build.sourcemap` is tied to `TAURI_ENV_DEBUG` | [start/frontend/vite](https://v2.tauri.app/start/frontend/vite/) |
| T-11 | `app.withGlobalTauri` is `false`, so the API is not on `window.__TAURI__` | present (default `false`) | `jq '.app.withGlobalTauri'` | [config #appconfig](https://v2.tauri.app/reference/config/#appconfig) |
| T-12 | The single-instance plugin is registered first, and its `argv`/`cwd` callback is treated as untrusted | absent | `rg single_instance src-tauri/src`; confirm plugin is the first registration | [plugin/single-instance](https://v2.tauri.app/plugin/single-instance/) |
| T-13 | Deep links use statically configured schemes only, and URL shape is validated after receipt | absent (no deep-link plugin) | `rg deep_link src-tauri/src`; review validation of scheme, host, and path against an expected format | [plugin/deep-linking](https://v2.tauri.app/plugin/deep-linking/) |
| T-14 | Updater artifacts are signed, the public key is pinned, endpoints are HTTPS, and `dangerousInsecureTransportProtocol` is not set | absent (no updater) | `jq '.bundle.createUpdaterArtifacts, .plugins.updater'` | [plugin/updater](https://v2.tauri.app/plugin/updater/) |
| T-15 | Installers and app bundles are code-signed for each target platform | unknown | `signtool verify /pa` (Windows) or `codesign -dv --verbose=4` (macOS) on the release artifact | [security/lifecycle](https://v2.tauri.app/security/lifecycle/) |
| T-16 | `app.security.headers` supplies `nosniff`, `Referrer-Policy`, and anti-caching headers | absent | `jq '.app.security.headers'`; confirm headers appear on document responses in devtools | [config #securityconfig](https://v2.tauri.app/reference/config/#securityconfig) |
| T-17 | The opener permission scope is narrower than `opener:default` | absent (grants `opener:default`) | Compare the capability to the opener default set, which permits `http://`, `https://`, `mailto:`, `tel:`, and reveal-in-directory | [security/permissions](https://v2.tauri.app/security/permissions/) |

The **Observed state** column records what is in the tree today and is not a verdict. `absent` means the
control is not configured or the mechanism is not present; `partial` means present but narrower or broader
than the control describes; `unknown` means it cannot be decided without inspecting a release artifact.

## 6. Overlap and gaps

**Where the two sources overlap** (ASVS requirement satisfied *by* a Tauri mechanism):

- **3.4.3 ↔ T-01/T-02/T-03.** Tauri injects nonces and hashes for bundled scripts and styles at build time,
  which is the practical form of the CSP requirement in a webview. ASVS's L3 per-response nonce model has no
  analogue here.
- **13.4.2 ↔ T-09/T-10.** "Debug modes disabled in production" maps exactly onto the `devtools` feature flag
  and on shipping source maps.
- **15.2.3 ↔ T-09/T-10/T-11.** "No extraneous production functionality" is the same control described from the
  build-content side.
- **15.3.6 ↔ T-08.** Tauri ships a purpose-built prototype-pollution mitigation (`freezePrototype`).
- **8.2.1, 13.2.2, 13.2.4 ↔ T-04/T-05/T-17.** Function-level authorization becomes capability plus command
  scope; egress allowlisting becomes CSP source lists plus opener scope.
- **2.2.2, 8.3.1 ↔ T-06.** "Trusted service layer" is the Rust command handler.
- **3.4.4, 3.4.5, 14.3.2 ↔ T-16.** Header requirements can be met through `app.security.headers`.

**Where Tauri fills a gap ASVS does not cover.** Three controls have no ASVS L2 requirement behind them:

1. **Native IPC authorization (G1).** ASVS's only API chapter (V4) is HTTP shaped and was dropped in §4.
   Nothing in ASVS asks a reviewer to enumerate privileged native commands, bind each to a caller, or test
   that a compromised webview cannot reach unlisted commands. Tauri supplies capabilities, permissions,
   command scoping (`AppManifest::commands`), and the isolation pattern. This is the single largest uncovered
   area: Lockr exposes 30 commands to one window with no command-level allowlist.
2. **Desktop artifact and update integrity (G2).** ASVS 5.0 mentions application-update validation only in
   the V15.3 *prose* ("the risk of not cryptographically validating application updates"); there is no numbered
   L1/L2 requirement for it. Tauri requires a signature on every update artifact ("This cannot be disabled"),
   pins a `pubkey` in configuration, enforces TLS on update endpoints, and exposes
   `dangerousInsecureTransportProtocol` as an explicit opt-out. Installer code signing is likewise only prose
   in ASVS.
3. **Operating-system entry points (G3).** Deep links, launch `argv`, and second-instance activation are ways
   for another local process to inject data into the app. ASVS has no chapter for this; 3.7.2 covers only
   in-app redirects. Tauri documents static-scheme matching, the single-instance-first ordering, and warns
   that "the user could trigger a fake deep link manually by including the URL as argument", so the app must
   still validate URL shape.

**Where the ASVS covers something Tauri does not:** V11 cryptography, V14 data classification, V16 logging
and error handling, V2 input validation, and V6 authentication quality have no Tauri guidance beyond generic
lifecycle advice. A checklist built only from Tauri docs would miss the product's core confidentiality claims.

## 7. Partial verdicts that a decision would settle

- **6.3.3 (MFA):** whether the public security claim is literal ASVS L2 or a tailored local-first profile,
  and whether OS account authentication plus device possession counts as a second factor. This one changes
  whether Lockr can claim L2 at all.
- **6.2.12 (breached passwords):** whether any breach check may make a network call, which conflicts with the
  local-first premise.
- **8.2.2 / 8.3.1:** whether multiple profiles on one device are a supported, security-relevant feature. The
  command set (`get_users`, `delete_user`, `login_user`) suggests yes, which makes IDOR/BOLA real.
- **5.x:** whether import/export ships in 1.2.0. If not, four file-handling rows leave scope entirely.
- **7.3.2:** whether "session" means an unlocked vault or the app process; the answer determines whether an
  absolute lifetime requirement is satisfiable at all.
