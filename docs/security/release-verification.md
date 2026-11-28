# Release verification

What the release-side checks can and cannot establish. Companion to
`audit-checklist.md` (T-09, T-15, 13.4.1, 13.4.2) and `security-level.md`.

## 1. Build

`yarn tauri build` at `bdea025` (2026-11-28; the audit re-run is `a41cde1`, and
the only delta is two dead-code annotations). The build succeeded:

- `src-tauri/target/release/lockr.exe`
- `src-tauri/target/release/bundle/msi/Lockr_1.2.0_x64_en-US.msi` (4,419,584 bytes)
- `src-tauri/target/release/bundle/nsis/Lockr_1.2.0_x64-setup.exe` (3,073,603 bytes)

SHA-256:

| Artefact | SHA-256 |
|---|---|
| MSI | `9D5889B462414E3EFE87FA1E2908B2B05C5BC89114EA86E7D9ADCB685360230B` |
| NSIS | `F83FE200490702B87ED48C8E48205F524352E1E84FDA22C4D0C07DCA3648424C` |

## 2. Signing (T-15) — not met

Both artefacts report `NotSigned` from `Get-AuthenticodeSignature`; there is no
signer certificate. T-15 remains **unchecked**. Closing it needs a code-signing
certificate and a signed build; the source tree cannot supply that.

## 3. What the build does verify

- **Source maps (13.4.2, partly):** `dist/` contains no `*.map` and no
  `sourceMappingURL`, and `static:bundle-sourcemaps` passes on the built
  bundle. `build.sourcemap` follows `TAURI_ENV_DEBUG`, so a release build does
  not enable them.
- **Devtools feature (T-09, source side):** `src-tauri/Cargo.toml` has no
  `devtools` feature and `static:devtools-absent` passes. The release binary
  built without it.
- **Bundle secrets:** `static:bundle-secrets` finds no `logo.dev`,
  `VITE_LOGO_API_TOKEN`, seed fixture or test token in `dist/`.

## 4. What remains manual

| Row | Why the artefact does not close it |
|---|---|
| T-09 | Confirming no inspector opens needs launching the installed release and attempting to open devtools; the source-side feature check is not a launch test |
| 13.4.1 | Enumerating the installer payload needs `lessmsi`/`7z` (neither installed here); `dist/` has no VCS metadata or maps, and the installer is Tauri-generated from the release exe, but the payload was not inspected |
| 13.4.2 | A launch check of the installed release is needed to confirm debug mode is off; the build-side sourcemap assertion passes |
| T-15 | Unsigned artefacts; a certificate and signed build are required |

The audit rung stays **assessed against L2** until these are closed.

## 5. Reproduce

```powershell
yarn tauri build
Get-AuthenticodeSignature src-tauri/target/release/bundle/msi/*.msi, src-tauri/target/release/bundle/nsis/*.exe
Get-FileHash src-tauri/target/release/bundle/msi/*.msi, src-tauri/target/release/bundle/nsis/*.exe -Algorithm SHA256
yarn test:compliance
```
