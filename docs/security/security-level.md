# Security level

This document states what security level Lockr claims, against what, and how that was measured. It
is written to be read by someone who does not trust the application — a User, an auditor, or a
reviewer deciding whether to install it.

**It is a self-assessment, not a certification.** No third party has assessed Lockr. Every claim
below points at a checklist row, which points at evidence.

Decision record for [#72](https://github.com/DylanAlmond/lockr/issues/72) (parent
[#63](https://github.com/DylanAlmond/lockr/issues/63)).

## 1. Scope

| | |
|---|---|
| **Product** | Lockr 1.2.0, Tauri 2 desktop application |
| **Assessed surface** | The Vue renderer and the renderer↔Rust seam: `invoke`, Tauri configuration, capabilities, and the shipped bundle |
| **Not assessed** | Rust cryptography and vault-format correctness (ADR-0001), the OS, the webview engine, and anything requiring memory forensics |
| **Not present** | No sync, no Device pairing, no network listener, no updater, no deep links |
| **Pinned to** | commit `<sha>` — a claim is valid only for the commit it names |
| **Method** | OWASP ASVS 5.0 Level 2, client tier, plus a Tauri 2 hardening supplement |

"Level 2" means **all applicable L1 and L2 requirements**, not a subset. That is why the claim is
split into a target and a status.

### Allowed communication

The complete egress allowlist. Production builds make **no outbound HTTP, DNS or TLS request**;
the CSP restricts the renderer to the entries below.

| Destination | When | Purpose |
|---|---|---|
| `ipc: http://ipc.localhost` | always | renderer ↔ Rust core (`invoke`) |
| App protocol, `'self'` | always | bundled scripts, styles, font, bundled SVG |
| `data:` images | always | icon uploads (128 × 128 PNG) |
| `ws://localhost:1420`, `ws://localhost:1421` | **development only** | Vite HMR, allowed by `devCsp` |

No other destination is permitted: `connect-src` is IPC-only, `img-src` is
`'self' data:`, and the `opener`, `fs`, `shell` and `http` plugins are not
installed. The bundle's only external string literals are `http://www.w3.org`
(the SVG namespace) and `https://vuejs.org` (Vue error text); neither is fetched.

## 2. The claim

**Target:** ASVS 5.0 Level 2, client tier, plus the Tauri supplement recorded in
`docs/security/research/audit-baseline.md`.

**Status:** *assessed against L2.* The conformance sweep is complete against `f3d5c82` on 2026-11-27.
**No ASVS level is claimed** — a compliance claim needs zero open deviations, and the sweep found one
P0, two P1 and a long P2/P3 tail.

**Remediation status (2026-11-28):** all 29 findings are fixed; every finding in `findings.md` now
names its commit and tests. This does **not** move the rung: the checklist has not been re-run against
the remediated tree, and the manual rows (installer signing, release devtools, OS facts) have not been
re-verified. Re-running `audit-checklist.md` and re-pinning this document is the release task.

| Rung | Meaning |
|---|---|
| **In assessment** | The sweep is not complete. No level is claimed. |
| **Assessed against L2** | The sweep is complete against a pinned commit; every deviation is enumerated in `findings.md`. Still not a compliance claim. **← Lockr is here** |
| **Compliant within a named profile** | Zero open deviations. Self-assessed. |
| **Independently certified** | A third party has assessed it. Not claimed, not planned. |

The target does not move. The status moves as deviations close or open, and the document is
re-pinned on every release.

## 3. How it was measured

1. **Baseline** — the applicable ASVS 5.0 L1/L2 rows and Tauri controls were extracted, with every
   dropped requirement and its reason recorded (`research/audit-baseline.md`).
2. **Conformance sweep** — each applicable row is marked met, partial, deviated, or N/A-with-reason
   in `audit-checklist.md`.
3. **Adversarial pass** — the threat model's in-scope adversaries are exercised against the renderer,
   and findings are recorded with the citation format in `threat-model.md` §11.
4. **Evidence** — governed by `evidence-standard.md`; automated checks are preferred wherever a
   control can be asserted mechanically, and the suite that implements them is specified in
   `compliance-tests.md`.

## 4. Findings

Across **105 checklist rows**: 40 met, 17 partial, 41 deviated, 6 not applicable, 1 unobservable
(installer signing, which needs a signed release artefact).

The 41 deviated rows resolve into **29 findings**, in the project's severity scheme, not CVSS:

| Severity | Count | Findings |
|---|---|---|
| **P0** — renderer compromise → vault read/write/exfiltration | 1 | F-14 |
| **P1** — secret exposure to a local observer, or vault-derived data leaving the machine | 2 | F-09, F-12 |
| **P2** — hardening gap that raises the likelihood of P0/P1 | 11 | F-01–F-04, F-10, F-11, F-13, F-15, F-18, F-19, F-28 |
| **P3** — hygiene | 15 | F-05–F-08, F-16, F-17, F-20–F-27, F-29 |

The three that matter most:

- **F-14 (P0) — any vault file can be deleted by id.** No ownership check, no unlock requirement, no
  re-authentication; `list_vault_ids` enumerates every vault file on disk. A compromised renderer can
  destroy every User's vault on the machine.
- **F-09 (P1) — vault-controlled icon URLs are fetched with no scheme allowlist.** Merely listing an
  item can send a request to an attacker-chosen host, leaking IP and user agent.
- **F-12 (P1) — no command checks `vault_id ∈ active_user.vault_ids`.** Latent while only one User's
  vaults are unlocked, and a cross-User read path in combination with F-13.

**Post-remediation:** each of these is fixed in `findings.md` with its commit and tests. The counts
and examples above describe the pinned `f3d5c82` sweep, not the current tree.

**Notable absence:** there is no HTML, script, navigation or `postMessage` sink in the renderer. The
audit found no live cross-site-scripting path, which is why the missing CSP is P2 rather than P0.

Full detail, evidence and status per finding: `findings.md`. Per-row status: `audit-checklist.md`.

### 6.3.3 in plain words

Lockr's only factor is the master password. The 32-byte secret key is stored encrypted under that
password, so it adds work to an offline attack but is not independent possession. OS login does not
gate the vault: anyone with the User's open session can read it.

**Consequence:** a stolen master password plus a stolen vault file is sufficient to decrypt the
vault. There is no second factor to stop it.

## 5. What a reader should not infer

- **Not certified.** No external assessor has reviewed this. "Assessed against L2" is a statement
  about method and scope, not a certificate.
- **Not a statement about the OS.** Per-user file permissions, installer signing, and the webview
  engine are assumed, not verified here (threat model AS1–AS5).
- **Not a statement about cryptography.** Use of Argon2id and AES-256-GCM is recorded as a
  configuration fact; no cryptanalysis was performed.
- **Not a statement about memory.** JavaScript strings cannot be zeroized; renderer secrets are
  reachable-limited, not erased. The Rust core's `zeroize` use is separate and real.
- **No network surface today.** Absence of sync, updater and deep links is recorded so a future
  addition cannot inherit a stale claim.

## 6. How to check this document

Each claim cites a checklist row; each row names its evidence, automated where possible. The
checklist is re-runnable: run it against a commit and the diff shows what changed. If a claim here
does not lead to a row, the claim is wrong and should be removed.

## 7. Change log

| Date | Commit | Change |
|---|---|---|
| 2026-11-27 | `f3d5c82` | Document created; target set |
| 2026-11-27 | `f3d5c82` | Conformance sweep complete; status = assessed against L2; 29 findings recorded |
| 2026-11-28 | remediation branches | All 29 findings fixed; assessment not re-pinned; checklist re-run is the release task |
