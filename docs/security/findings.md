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

## Pending

Groups 2–7: renderer injection and secret exposure; IPC and authorisation; authentication and
session; cryptography, files and data protection; supply chain and release hygiene; logging and
error handling.
