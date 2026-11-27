# Tauri security configuration baseline

Recommendation for [#70](https://github.com/DylanAlmond/lockr/issues/70) (parent [#63](https://github.com/DylanAlmond/lockr/issues/63)).
This is the diff-ready configuration and the reasoning for every non-default line. **Nothing here is
applied**; fixes are out of scope for the audit map, and each line below becomes a fix ticket.

Sources: Tauri 2 CSP docs (<https://v2.tauri.app/security/csp/>), the installed config schema
(`node_modules/@tauri-apps/cli/config.schema.json`, Tauri CLI for `tauri` 2.11.4), and the
`baseline`/`bundle`/`opener` evidence documents under `docs/security/research/`.

## 1. `src-tauri/tauri.conf.json`

Replace the whole `app.security` object with:

```json
"security": {
  "csp": {
    "default-src": "'self'",
    "script-src": "'self'",
    "style-src": "'self'",
    "img-src": "'self' data:",
    "font-src": "'self'",
    "connect-src": "ipc: http://ipc.localhost",
    "object-src": "'none'",
    "base-uri": "'none'",
    "frame-ancestors": "'none'",
    "form-action": "'none'"
  },
  "devCsp": {
    "default-src": "'self'",
    "script-src": "'self'",
    "style-src": "'self' 'unsafe-inline'",
    "img-src": "'self' data:",
    "font-src": "'self'",
    "connect-src": "ipc: http://ipc.localhost ws://localhost:1421 ws://127.0.0.1:1421",
    "object-src": "'none'",
    "base-uri": "'none'",
    "frame-ancestors": "'none'",
    "form-action": "'none'"
  },
  "freezePrototype": true,
  "headers": {
    "X-Content-Type-Options": "nosniff",
    "Cross-Origin-Resource-Policy": "same-origin",
    "Cross-Origin-Opener-Policy": "same-origin",
    "Permissions-Policy": "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=()"
  },
  "pattern": { "use": "brownfield" }
}
```

`dangerousDisableAssetCspModification` is **omitted on purpose**: its default `false` lets Tauri
inject the nonces and hashes for bundled scripts and styles, which is what makes `script-src 'self'`
sufficient.

## 2. Why each line

| Directive | Why | Discharges |
|---|---|---|
| `default-src 'self'` | Default-deny for anything not named below. The renderer loads only bundled assets. | ASVS 3.4.3, T-01 |
| `script-src 'self'` | No inline scripts, no `eval`, no remote scripts. The production bundle contains zero `eval`/`new Function` and Tauri hashes the bundled entries. | 1.3.2, 3.2.2, T-01 |
| `style-src 'self'` | **The eight `:style` bindings do not need `'unsafe-inline'`.** Vue writes them through the CSSOM (`element.style.setProperty`), which CSP does not govern; only inline `style` attributes and `<style>` elements are restricted. Production CSS is a bundled file. | 3.4.3 |
| `img-src 'self' data:` | Icons are canvas-produced `data:` URLs. **A remote icon URL is now blocked**, which closes the beacon channel: a crafted vault or profile can no longer make the app fetch an attacker host. | 13.2.4, 14.2.3 |
| `font-src 'self'` | The only font is the bundled Cal Sans woff2. | 13.2.4 |
| `connect-src ipc: …` | **Required.** Tauri's IPC on Windows is an `http://ipc.localhost` fetch; without it `invoke` fails. This is the sole permitted egress. | 13.2.4, T-01 |
| `object-src 'none'` | No plugins or `<object>`; removes a classic script-execution vector. | 3.4.3 |
| `base-uri 'none'` | No `<base>` rewriting, which would otherwise retarget every relative script URL. | 3.4.3 |
| `frame-ancestors 'none'` | The window cannot be embedded. | 3.4.3 |
| `form-action 'none'` | No form may submit anywhere; there is no server. | 3.4.3, 13.2.4 |
| `freezePrototype: true` | Freezes `Object.prototype` on the custom protocol. Nothing in `src/` requires a mutable prototype; the renderer parses backend-shaped JSON. | 15.3.6, T-08 |
| `X-Content-Type-Options: nosniff` | Stops MIME sniffing on protocol responses. | 3.4.4, T-16 |
| `Cross-Origin-Resource-Policy: same-origin` | No other origin may load the app's resources. | T-16 |
| `Cross-Origin-Opener-Policy: same-origin` | No opener relationship with another window. | T-16 |
| `Permissions-Policy` | Denies camera, microphone, geolocation, payment, USB, serial and HID to the webview. The app needs none. | 13.2.4, T-16 |

## 3. Two corrections to the grilling answers

The round accepted `Referrer-Policy: no-referrer` and `Cache-Control: no-store`. **Tauri 2.11.4
cannot set either** through `app.security.headers`: `HeaderConfig` is a closed set
(`additionalProperties: false`) and contains neither name. Writing them would make the config
schema-invalid.

Substitutes, and why the underlying requirements are still met:

- **ASVS 3.4.5 (referrer policy)** — residual referrer egress requires an outbound request. With
  `connect-src` and `img-src` closed and the opener capability removed (§4), the renderer has no
  path to make one. The requirement is met by exclusion rather than by a header. Record it that way
  in the checklist; do not record it as satisfied by configuration.
- **ASVS 14.3.2 (anti-caching)** — the custom protocol serves only static bundle assets. Vault data
  reaches the renderer over IPC, which is not an HTTP cache participant. No sensitive response
  exists to mark `no-store`. Record as not applicable, with that reason.

If a custom protocol handler is ever added that serves vault-derived bytes, both rows must be
revisited immediately.

## 4. Remove the opener capability

Delete `opener:default` from `src-tauri/capabilities/default.json`, remove
`tauri-plugin-opener` from `src-tauri/Cargo.toml` and its `.plugin(tauri_plugin_opener::init())`
registration in `src-tauri/src/lib.rs`, and drop `@tauri-apps/plugin-opener` from `package.json`.

There is no call site. Keeping it grants:

- `open_url` for any `http(s)`/`mailto`/`tel` string — OS handler dispatch from vault-controlled data;
- `reveal_item_in_dir` with **no scope at all** — arbitrary existing-path reveal, a filesystem
  existence oracle, and UNC/NTLM egress on Windows.

Removing the capability is the smallest change that deletes all three. A future "open this item's
URL" feature should arrive as a scoped permission with an explicit host allowlist.

## 5. Capability file

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Capability for the main window",
  "windows": ["main"],
  "permissions": [
    "core:event:default",
    "core:app:allow-version",
    "core:window:allow-close",
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-is-maximized",
    "core:window:allow-start-dragging"
  ]
}
```

Each entry has a call site: `core:event` for `onResized`; `app:allow-version` for `AboutAppModal`'s
`getVersion`; the window permissions for `Titlebar.vue` and `data-tauri-drag-region`. `core:default`
is dropped because it also bundles menu, tray, image, resources and path permissions the app never
uses. `invoke` of app-defined commands needs no entry.

## 6. Isolation pattern: declined, recorded

`pattern` stays `brownfield`. Isolation would sandbox IPC behind an iframe that validates every
message, which is real defence-in-depth. It is declined **for now** because:

- the threat model's R2 already requires the Rust core to validate every argument, and the missing
  ownership check is the actual gap — isolation would relocate it, not close it;
- it breaks direct `@tauri-apps/api` usage and needs a compatibility pass of its own.

Revisit if the ownership check proves impractical, or before any feature renders third-party
content.

## 7. Build-side settings

```ts
// vite.config.ts
build: {
  sourcemap: !!process.env.TAURI_ENV_DEBUG
}
```

- Source maps follow the Tauri debug flag instead of Vite's default, so a release build cannot
  accidentally ship them.
- `devtools` stays absent from `tauri` features. The checklist asserts a release build exposes no
  inspector, rather than trusting the default.

## 8. What this baseline does not fix

The CSP prevents a future injection from reaching the IPC surface, but it does not constrain a
renderer that is *already* compromised: that renderer can still call all 30 commands. The
ownership check (`vault_ids ∈ active_user`) and the secret lifetime policy remain the controls that
bound a compromised renderer. This document is one layer, not the answer.

## 9. Verification

1. `npx tauri dev` — app loads, HMR connects, `invoke` works, no CSP violations in the console.
2. `npx tauri build` — launch the release artifact; `invoke` works, font loads, icons render.
3. Open devtools in the release build — must not open.
4. Search `dist/` for `*.map` — must be empty.
5. In the running app, evaluate `Object.isFrozen(Object.prototype)` — must be `true`.
6. Attempt `fetch('https://example.com')` from the console — must be blocked by CSP.
7. Set a vault icon to `https://example.com/x.png`, reopen it — the image must not load.
8. In the console, invoke any opener command — it must be rejected by the ACL.
