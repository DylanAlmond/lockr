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

The release pipeline is now wired to sign on all three platforms and to fail
rather than publish an unsigned artefact (`release-signing.md`): Windows
Authenticode with an RFC 3161 timestamp, macOS Developer ID with notarization
and stapling, and Linux GPG signatures over an AppImage and a `SHA256SUMS`
manifest. The local build above is deliberately unsigned, so it cannot close
the row. `static:release-signing-config` checks that the workflow still carries
the guards, the verification scripts and the Windows digest/timestamp config.

To close T-15:

1. Configure the secrets listed in `release-signing.md` §2.
2. Run the `publish` workflow (push to `release`, or dispatch it manually).
3. Record the signed artefacts and the verifier output in this document.

Until step 3, the row stays unchecked.

### Development-certificate dry run

To prove the build → sign → verify path before buying a certificate, the tree was built at `d001c13`
with the self-signed certificate from `scripts/create-dev-certificate.ps1`
(`CN=Lockr, O=Dylan Almond`). `scripts/verify-windows-signatures.ps1` reported:

```text
Lockr_1.2.0_x64_en-US.msi: Valid (timestamped)
Lockr_1.2.0_x64-setup.exe: Valid (timestamped)
MSI payload lockr.exe: Valid (timestamped)
All 3 Windows release artefacts carry a valid, timestamped signature.
```

The signatures use SHA-256 and a DigiCert RFC 3161 timestamp. The signed artefacts were:

| Artefact | SHA-256 |
|---|---|
| MSI | `DA7245F5C31F0402E40A4D07551EC31492AEB196BA065852F08B65FA21A949BC` |
| NSIS | `9CDDC48557D1FC6116EEDBF5002CB83E028E0C3822BAFFF4CDCE58407A63998F` |

This proves the mechanism, not the trust: the certificate is self-signed and trusted only on the
build machine, so Windows on any other machine still reports an unknown publisher. The pre-bundle
`target/release/lockr.exe` is unsigned by design; the verifier checks the installers and the MSI
payload instead. T-15 remains unchecked until the same run uses a CA-issued certificate or Azure
Trusted Signing.

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
| T-15 | Unsigned release artefacts. A self-signed dry run signs and verifies correctly, but the certificate is not trusted off the build machine; a CA-issued certificate or Azure Trusted Signing is still required |

The audit rung stays **assessed against L2** until these are closed.

## 5. Reproduce

```powershell
yarn tauri build
pwsh scripts/verify-windows-signatures.ps1
Get-FileHash src-tauri/target/release/bundle/msi/*.msi, src-tauri/target/release/bundle/nsis/*.exe -Algorithm SHA256
yarn test:compliance
```

Signed releases are produced only by `.github/workflows/publish.yml`; follow
`release-signing.md` §4 for the per-platform verification commands.
