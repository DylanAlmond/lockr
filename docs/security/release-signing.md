# Release signing

Decision record for control **T-15** (installers and app bundles are code-signed for each target
platform) and the release half of the audit. Companion to `release-verification.md` and the T-15 row
in `audit-checklist.md`.

## 1. What is signed, and by what

| Platform | Artefacts | Mechanism | Verification |
|---|---|---|---|
| Windows | `lockr.exe`, NSIS `-setup.exe`, `.msi` | Authenticode, SHA-256 digest, RFC 3161 timestamp | `scripts/verify-windows-signatures.ps1` |
| macOS | `.app`, `.dmg` | Developer ID Application, hardened runtime, notarization + stapling | `scripts/verify-macos-signatures.sh` |
| Linux | `.AppImage`, `.deb`, `.rpm` | GPG: embedded AppImage signature, plus a detached signature over `SHA256SUMS` | `scripts/verify-linux-signatures.sh` |

Linux has no OS-enforced installer signing. The AppImage signature is the platform's native
mechanism but is **not checked by the OS**; the detached `SHA256SUMS.asc` is what covers the `.deb`
and `.rpm`, and both require the user to verify against the published key. That limitation is why
the Linux leg is not equivalent to Authenticode or Developer ID.

## 2. Where the signing material comes from

The repository never contains a certificate or private key. The release workflow
(`.github/workflows/publish.yml`) reads them from GitHub Actions secrets and **fails the job when a
secret is missing**, so an unsigned build never reaches a release draft.

| Secret | Platform | Format |
|---|---|---|
| `WINDOWS_CERTIFICATE` | Windows | Base64 of the `.pfx` (`certutil -encode`) |
| `WINDOWS_CERTIFICATE_PASSWORD` | Windows | The `.pfx` export password |
| `APPLE_CERTIFICATE` | macOS | Base64 of the exported `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | macOS | The `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | macOS | `Developer ID Application: … (TEAMID)` |
| `APPLE_ID` | macOS | Apple ID used for notarization |
| `APPLE_PASSWORD` | macOS | App-specific password for that Apple ID |
| `APPLE_TEAM_ID` | macOS | Apple Developer team id |
| `LINUX_GPG_PRIVATE_KEY` | Linux | Base64 of the ASCII-armored private key |
| `LINUX_GPG_KEY_ID` | Linux | Full fingerprint of the signing key |
| `LINUX_GPG_PASSPHRASE` | Linux | Passphrase for the signing key |

Notarization can use an App Store Connect API key instead of `APPLE_ID`/`APPLE_PASSWORD`; if that is
chosen, replace those three secrets with `APPLE_API_ISSUER`, `APPLE_API_KEY` and
`APPLE_API_KEY_PATH` and update the guard step to match.

### Development certificate

`scripts/create-dev-certificate.ps1` creates a self-signed code-signing certificate with subject
`CN=Lockr, O=Dylan Almond`, trusts it on the current machine and writes
`windows-signing.local.json`. It exists to exercise the build → sign → verify path locally. It is
**not** a release credential: other machines do not trust it, and it does not satisfy T-15. The
script prints the commands that remove the certificate and the local config.

## 3. How the pipeline signs

1. **Guards.** Each platform has a step that errors with `must be set for a release build (T-15)`
   when its secrets are empty.
2. **Windows.** The `.pfx` is decoded and imported into the runner's certificate store; the
   thumbprint is written to `windows-signing.conf.json`, which `tauri build` merges with
   `--config`. The repository's `tauri.conf.json` pins `digestAlgorithm: sha256`, the DigiCert
   timestamp URL and `tsp: true`; the thumbprint stays out of the tree because it is
   environment-specific.
3. **macOS.** The Tauri CLI reads `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD` and
   `APPLE_SIGNING_IDENTITY`, signs the `.app`, notarizes it with the Apple ID credentials and
   staples the ticket. `hardenedRuntime` is on.
4. **Linux.** The build runs with `SIGN=1`, `SIGN_KEY` and `APPIMAGETOOL_SIGN_PASSPHRASE`, and
   `APPIMAGETOOL_FORCE_SIGN=1` so a signing failure fails the build. After the build,
   `scripts/sign-linux-assets.sh` writes `SHA256SUMS`, signs it with the release key and uploads the
   manifest, signature and public key to the draft release.
5. **Verify.** The workflow then runs the platform verifier. A non-zero exit fails the job and the
   draft is not published.

## 4. Verifying a release by hand

```powershell
# Windows — every .exe/.msi must report Valid and carry a timestamp
pwsh scripts/verify-windows-signatures.ps1
Get-AuthenticodeSignature src-tauri/target/release/bundle/**/*.msi
```

```bash
# macOS — signature, Gatekeeper assessment and stapled ticket
bash scripts/verify-macos-signatures.sh
codesign -dv --verbose=4 "/Applications/Lockr.app"
spctl --assess --type execute --verbose=4 "/Applications/Lockr.app"
xcrun stapler validate Lockr_1.2.0_aarch64.dmg
```

```bash
# Linux — checksums, detached signature, AppImage signature
bash scripts/verify-linux-signatures.sh
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum --check SHA256SUMS
```

For a downloaded release, import the public key first (`lockr-release-signing-key.asc` is attached
to the release) and confirm its fingerprint against the one published in the repository before
trusting the signature.

## 5. Status and what is still missing

**T-15 is not met.** The pipeline is implemented and statically checked
(`static:release-signing-config`), and an unsigned local build was verified as unsigned in
`release-verification.md`. A control that says "installers are code-signed" is only met by a
**signed artefact**, and no signing material exists yet:

- no Windows OV/EV code-signing certificate;
- no Apple Developer ID certificate or notarization credentials;
- no Linux release GPG key.

Once the secrets are configured, push the `release` branch (or run the workflow manually), let the
three platform jobs verify and upload, and record the signed artefacts — hashes, signer identities
and the `Get-AuthenticodeSignature`/`codesign -dv`/`gpg --verify` output — in
`release-verification.md`. Only then does the T-15 row move from unchecked to met, and only then can
the audit rung be reconsidered.

## 6. Key custody and rotation

- Certificates and keys live only in the GitHub Actions secret store and the owner's offline
  backup; never in the repository, a build log, or a release asset.
- The Windows certificate is imported into the ephemeral runner store and the `.pfx` is deleted in
  the same step.
- The Linux private key is imported into a temporary GPG home and the decoded file is removed.
- On rotation: replace the secret, re-run the release build, publish the new public key and record
  the new fingerprint. A compromised key means revoking the certificate and re-signing the last
  release; the old signatures must not be represented as trustworthy afterwards.
- Signing happens only in the release workflow. Local `yarn tauri build` remains unsigned by design,
  so a local artefact must never be published as a release.
