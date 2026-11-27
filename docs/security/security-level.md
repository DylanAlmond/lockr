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

## 2. The claim

**Target:** ASVS 5.0 Level 2, client tier, plus the Tauri supplement recorded in
`docs/security/research/audit-baseline.md`.

**Status:** *in assessment.* No ASVS level is claimed at the time of writing.

| Rung | Meaning |
|---|---|
| **In assessment** | The sweep is not complete. No level is claimed. **← Lockr is here** |
| **Assessed against L2** | The sweep is complete against a pinned commit; every deviation is enumerated below. Still not a compliance claim. |
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
4. **Evidence** — governed by `evidence-standard.md`; automated tests are preferred wherever a
   control can be asserted mechanically.

## 4. Known deviations at the time of writing

These are already established by the research and threat model. The completed sweep will replace
this list with the full set, including anything new it finds.

| ASVS | Requirement | State | Why |
|---|---|---|---|
| **3.4.3** | CSP with `object-src`/`base-uri` and an allowlist | **Deviated** — `csp: null` | Fix specified in `tauri-config-baseline.md` |
| **8.2.2 / 8.3.1** | Data-specific and layer-enforced authorisation | **Deviated** — no command checks `vault_id ∈ active_user.vault_ids` | The core gates on "vault unlocked", not "vault owned"; `vault_ids` is never consulted outside login |
| **7.3.1** | Inactivity timeout forces re-authentication | **Deviated** — no idle lock | A session lasts until logout or exit |
| **7.4.2** | Terminating an account terminates its sessions | **Deviated** — `login_user` and `delete_user` do not end prior sessions | Prior vaults stay readable in RAM |
| **14.3.1** | Authenticated data cleared when the session ends | **Deviated** — renderer caches secrets with no expiry | Policy specified in `secret-lifetime.md` |
| **16.3.1 / 16.3.2** | Authentication and authorisation events are logged | **Deviated** — no log sink | Neither success nor failure is recorded |
| **6.2.1** | Minimum password length | **Deviated** — registration accepts any password | The strength meter exists but registration never calls it |
| **6.2.4** | Common-password check | **Deviated** — zxcvbn is advisory only | Not enforced as a gate |
| **6.2.12** | Breached-password check | **Standing deviation** | A breach lookup needs a network call, contradicting the local-first premise |
| **6.3.3** | MFA, or a combination of single factors | **Standing deviation** — single factor | The secret key is encrypted under the master password, so it is not independent; OS session authentication does not gate vault access |
| **2.4.1 / 6.3.1** | Anti-automation and lockout | **Deviated** — no attempt limiting | Repeated `login_user` calls are unbounded |
| **13.2.4** | Allowlist of external resources | **Deviated** — vault-controlled icon URLs can fetch | Closed by the `img-src 'self' data:` directive in the config baseline |

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
| 2026-11-27 | `<sha>` | Document created; target set; status = in assessment |
