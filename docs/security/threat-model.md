# Renderer threat model

**Status:** accepted 2026-11-27 · **Scope:** Lockr 1.2.0, the Vue renderer and the renderer↔Rust
seam · **Companions:** `docs/security/audit-checklist.md`, `docs/security/findings.md`

Every finding in this audit must cite a node from this document: an asset (`A#`), an entry point
(`E#`), and an adversary (`ADV#`). A finding that cannot name all three is not yet a finding.

Vocabulary follows `GLOSSARY.md` — **Vault**, **Item**, **Item Kind**, **Field Key**, **Safe View
Pattern**. There is no Device, Pairing or sync in 1.2.0; see §10.

---

## 1. What the product is, as built

- A Tauri 2 desktop app with one window (`main`), a Vue 3 renderer, and a Rust core.
- Each **Vault** is one encrypted file (`AES-256-GCM`), keyed from
  `Argon2id(master password ‖ secret key, salt)`.
- **Multiple Users per installation.** Each User has a name, icon, colour, a master password, a
  secret key, and `vault_ids` — documented in code as "IDs of all vaults this user owns"
  (`src-tauri/src/models.rs:87-99`). One User is active at a time.
- `login_user` caches the derived secret key and unlocks **every** vault in the active User's
  `vault_ids` (`src-tauri/src/commands.rs:117-153`).
- No network code: no sync, no Device pairing, no updater, no deep links, no single-instance
  plugin. The sync ADR and its vocabulary were reverted before any implementation landed.

The README makes three security claims this model exists to test:

1. "The Vue frontend acts strictly as a 'dumb' display layer, ensuring sensitive data is handled
   safely."
2. "Memory Safety: uses the `zeroize` crate … passwords and encryption keys are securely wiped
   from RAM when vaults are locked or swapped."
3. "Local-First & Offline: No servers, no cloud, no network requests."

---

## 2. Assets

Tiers: **T1** compromise means immediate credential loss · **T2** means targeting or enumeration ·
**T3** means reconnaissance.

| ID | Asset | Tier | Where it lives today |
|---|---|---|---|
| **A1** | Master password | T1 | Typed into the renderer by construction; cached in Rust RAM (`master_password` state) while any session is open |
| **A2** | User secret key (32 bytes) | T1 | Rust RAM (`secret_key` state). The *encrypted* blob is returned to the renderer via `get_users`/`get_user` (`models.rs:87-99`) |
| **A3** | Secret field values — account `password`; card `card_number`, `security_code`, `pin` | T1 | Decrypted vault JSON in Rust RAM. Only egress is `get_item_secret` (`commands.rs:467-482`) |
| **A4** | Item metadata — display name, username/email, URLs, tags, icon, colour, timestamps | T2 | Safe views in the renderer |
| **A5** | User and Vault metadata — user name, icon, colour, vault names, vault count, `encrypted_secret_key` | T2 | Renderer; profile JSON on disk is unencrypted |
| **A6** | Diagnostics — error strings, console output, dev fixtures, build artefacts | T3 | Renderer console, `src/seed.ts`, `dist/` |

Two classification choices are deliberate and should be re-examined if challenged:

- **`encrypted_secret_key` is an asset (A2), not packaging.** It is offline-guessing material for
  the master password and is handed to the renderer today. A renderer compromise therefore yields
  not just the unlocked session but a durable offline attack against every profile on the machine.
- **Note bodies are T2, not T1.** A note Item's body travels in safe views because the product
  models only *field keys* as secrets. That is a product decision, not a security property, and the
  audit records it as a model choice rather than an endorsement.

---

## 3. Trust zones and the boundary rule

| Zone | Contents | Trust |
|---|---|---|
| **Z1 Hostile input** | Vault files on disk, user profile JSON, uploaded images, clipboard contents pasted into the app | Untrusted |
| **Z2 Renderer** | Webview, Vue app, bundled JavaScript, npm runtime code | **Compromisable.** Not assumed hostile by default, but never relied upon |
| **Z3 Rust core** | `commands.rs`, managers, `crypto.rs`, models | Reference monitor |
| **Z4 Platform** | OS file permissions, webview engine, user session | Trusted by assumption — see §6 |

**Rule R1 — the boundary rule.** No security property may depend on the renderer behaving
correctly. Properties may depend only on Z3 and Z4.

**Corollary R1a.** Any property currently enforced only in Vue is a finding, even if no exploit is
known — the property is absent, not merely untested.

**Rule R2.** Z3 is trusted only for what it validates. An argument arriving over IPC is Z1 data
until the Rust side proves otherwise, regardless of what the renderer "would" send. Typed
deserialization proves shape, not authorisation.

---

## 4. Entry points

> Refreshed 2026-11-28 after remediation: 33 commands, a least-privilege capability with no opener,
> and clipboard copy through the core. The rows below describe the current tree.

| ID | Entry point | Reaches | Notes |
|---|---|---|---|
| **E1** | IPC — 33 commands registered in `lib.rs` | Z3 | The entire native surface. The capability grants the eight core permissions plus one `allow-<command>` per app command; no `fs`, `shell` or `http` plugin |
| **E2** | Vault file at rest | Z3 parsing, Z2 via safe views | Attacker-modified, restored from `.OLD`, or copied between machines |
| **E3** | User profile JSON | Z2 directly | Plaintext on disk: `id`, `name`, `color`, `icon`, `encrypted_secret_key`, `vault_ids`. Hand-editable |
| **E4** | Uploaded image | Z2 | PNG/JPEG/WebP/GIF up to 8 MiB, rasterised through a canvas to a 128 × 128 PNG `data:` URL |
| **E5** | Clipboard | Z2 outbound → OS | Copy goes through the core; the clipboard is cleared after 30 s if unchanged |
| **E6** | OS URL/file handlers | — | The opener plugin and capability are removed; no OS URL/file handler is exposed |
| **E7** | Migration backup (`.vault.OLD`) | Z3 | Rollback path; never read by the app |
| **E8** | Absent entry points | — | No network listener, no deep links, no updater, no `argv`/second-instance handling, no drag-and-drop import |
| **E9** | Vault tombstone (`.vault.deleted`) | Z3 | Renamed aside on delete; never read, listed, or restored by the app (ADR-0002) |

E8 is recorded on purpose: the audit asserts absence, so a later addition inherits the checklist
instead of silently widening the surface.

---

## 5. Adversaries

Ordered by weight. Severity follows this order, not CVSS.

| ID | Adversary | Can | Cannot | Assets at risk |
|---|---|---|---|---|
| **ADV1** | **Renderer compromise** — injected script, or a malicious package executing in the webview | Call any of the 30 commands with arbitrary arguments; read everything the renderer can read; copy to the clipboard; beacon out | Break AES-GCM; read the disk beyond what `fs` permissions allow (no such plugin is granted) | A1–A5 |
| **ADV2** | **Malicious vault or profile content** — a crafted, tampered, or restored file the User opens | Control every string in A4/A5: names, usernames, tags, icons, colours, note bodies; influence rendering | Execute code (no HTML sink exists today); call IPC | A4, A5; A3 only through what the User then does |
| **ADV3** | **Local observer** — another process in the User's OS session, or another OS user where permissions allow | Read the clipboard; capture the screen; read `app_data_dir`; read process memory of the same session | Decrypt vault files without the master password | A1–A5 |
| **ADV4** | **Supply chain** — a malicious or compromised npm or crate dependency, or a build step | Ship code into the bundle; at build time, inline environment values | Modify a release already installed and signed | A1–A5 |

**Explicitly out of scope as adversaries:** a network attacker (there is no network surface), disk
forensics or memory dumping at rest, a compromised OS or webview engine, and physical access to an
unlocked machine. These defeat any application-level control; pretending otherwise produces
findings no fix can satisfy.

---

## 6. Assumptions

Each assumption is either *held* (with how to check) or becomes a finding.

| ID | Assumption | How it is checked |
|---|---|---|
| **AS1** | The OS enforces per-user permissions on `app_data_dir` | Inspect the directory ACL on each target platform |
| **AS2** | The User's OS account is not shared with anyone they do not trust | Product assumption; stated in the public document, not testable in-app |
| **AS3** | Only one Lockr instance writes vault files at a time | No single-instance plugin exists; verify behaviour with two instances |
| **AS4** | The Rust core is memory-safe | `rg -n "unsafe" src-tauri/src` → no matches (holds at time of writing) |
| **AS5** | The webview engine is unmodified and current | OS-provided WebView2 / WKWebView; out of app control |
| **AS6** | Vault encryption and format correctness are not assessed here | ADR-0001 and the existing crypto tests are the authority |

AS2 and AS5 are the two the public security-level document must state plainly, because a reader
will otherwise assume the audit covered them.

---

## 7. Session model

A **session** is one unlocked Vault, owned by exactly one User.

- A User may have several Vaults; several sessions may be open at once, all owned by the active User.
- Locking a Vault ends that session. `logout` ends every session and zeroizes the cached master
  password and secret key.
- **Switching the active User must end every prior session.** This is a rule of the model; current
  code does not implement it — `login_user` overwrites the cached credentials without locking
  (`commands.rs:138-150`).
- A secret's lifetime is bounded by its Vault's session; the exact bounds belong to the
  secret-lifetime policy ticket.

Why this matters: "vault unlocked" is the only gate the core currently applies, so the definition of
a session *is* the authorisation lifetime.

---

## 8. Ownership model

- `User.vault_ids` is the access-control list: the only record of which Vaults a User owns.
- The caller identity is the active User.
- **Every** Vault- or Item-bearing command must establish `vault_id ∈ active_user.vault_ids`
  before acting.
- Today no command does. The gate is "Vault is present in the RAM map"
  (`vault_manager.rs:83-99`), not "Vault belongs to the caller". Under ADV1 — or after a User
  switch that did not end prior sessions — that is a cross-User read and write path.

This section is the direct consequence of treating multiple Users as supported. It is not a
hypothetical: the product ships a profile picker and a per-User `vault_ids` list.

---

## 9. Claims under test

| Claim | Source | Node that tests it |
|---|---|---|
| The renderer is a "dumb display layer" | README | R1; §8 — it is not, because authorisation lives there |
| Secrets are wiped on lock or swap | README | §7; A1–A3 |
| No network requests | README | E1, E4, A4 — vault-controlled icon URLs can fetch; the dormant logo.dev path would add a second |
| Safe views cannot carry secrets | `GLOSSARY.md` | A3/A4 — true for field keys; note bodies are not field keys |

---

## 10. Changes that invalidate this model

Revisit before relying on it if any of these lands:

- sync, Device pairing, or any network listener (re-introduces a network adversary and a third
  input zone);
- deep links, `argv` handling, single-instance activation (new Z1 entry points);
- an updater (artifact integrity becomes a control);
- import/export of vault files (new file-handling rows);
- sharing a Vault between Users (per-vault key wrapping; the current model has none);
- any HTML, markdown, or inline-SVG rendering in the renderer (would make ADV2 script-capable and
  promote the missing CSP to an exploit).

---

## 11. Finding citation format

```
F-<n>  [P0–P3]  A<asset>  ← E<entry>  ADV<n>
Rule/assumption broken: R1 | R2 | AS<n> | §7 | §8
Evidence: <file:line, command output, or reproduction steps>
```

Two examples of what the existing research already supports:

```
F-1  [P0]  A3, A4  ← E1  ADV1
Broken: §8, R2 — no command checks vault_id ∈ active_user.vault_ids
Evidence: vault_manager.rs:83-99; commands.rs:467-482
```

```
F-2  [P2]  A2  ← E3  ADV3
Broken: A2 exposure via get_users; encrypted_secret_key is offline-guessing material
Evidence: models.rs:87-99; user_manager.rs:42-65
```
