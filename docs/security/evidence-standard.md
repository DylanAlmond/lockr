# Evidence standard

What counts as proof in this audit, and what a finding must carry. Decision record for
[#72](https://github.com/DylanAlmond/lockr/issues/72) (parent
[#63](https://github.com/DylanAlmond/lockr/issues/63)). Companion to `security-level.md` and
`threat-model.md`.

The rule in one line: **a claim without evidence is an opinion, and an automated check beats a
manual one because it can be re-run.**

## 1. Evidence classes

| Class | What it is | Re-runnable |
|---|---|---|
| **E4 — Automated check** | A test or script that fails when the control regresses | Yes |
| **E3 — Reproduction** | Exact steps that demonstrate the behaviour, with observed output | Yes, by hand |
| **E2 — Captured output** | Command, tool output, or build artefact inspected | Partly |
| **E1 — Code citation** | `file:line` plus the reasoning that connects it to the claim | Yes, by reading |
| **E0 — Absence search** | The search run and its zero-hit result | Yes |

Absence is evidence. "No `v-html` in `src/` after `rg -n 'v-html' src`" is a legitimate finding the
document may cite; it is not weaker than pointing at a bad line, because the claim is about what does
not exist.

## 2. Minimum evidence by severity

| Severity | Minimum | Also required |
|---|---|---|
| **P0** | E1 + (E3 or E4) | The affected command or entry point, and the asset reached |
| **P1** | E1 + E3 | A concrete scenario that reaches the code path |
| **P2** | E1 | The entry point and the adversary that benefits |
| **P3** | E1 or E2 | The config value or artefact inspected |

Every finding also carries the threat-model citation — asset (`A#`), entry point (`E#`), adversary
(`ADV#`) — and a status from §3. The format lives in `threat-model.md` §11; this document defines
what fills it.

## 3. Status vocabulary

| Status | Meaning |
|---|---|
| `open` | Real, reachable, unfixed |
| `fix-specified` | The fix is decided in this map but not applied |
| `accepted` | A knowingly retained risk, with a stated reason and an owner |
| `deviated` | The ASVS row is not met; recorded in `security-level.md` §4 |
| `fixed` | Closed with the commit that closed it |
| `not-applicable` | Does not bind this stack, with the reason |

A finding never moves to `fixed` without a commit, and never to `accepted` without a reason a reader
can disagree with.

## 4. Compliance tests

Automated evidence (E4) is preferred wherever a control can be asserted mechanically. The point is
not coverage for its own sake: it is that the checklist can be re-run and the diff tells you what
regressed.

### 4.1 What can be asserted automatically

**Static — configuration and source (`scripts/compliance/*.mjs`, no framework needed)**

- `tauri.conf.json`: `csp` non-null; required directives present; `script-src` contains no
  `'unsafe-inline'`/`'unsafe-eval'`; `connect-src` is exactly `ipc: http://ipc.localhost`;
  `freezePrototype` is `true`; `devCsp` does not weaken production.
- Capability file: permission list equals the expected set; `opener:*` absent.
- `Cargo.toml`/`package.json`: no opener plugin; no `devtools` feature.
- Source search: zero hits for `v-html`, `innerHTML`, `eval(`, `new Function`, `localStorage`,
  `sessionStorage`, `postMessage` listeners, `window.open`, non-`data:` image sources.
- Lockfile: no git/URL/file resolutions; no non-default registry.

**Static — built artefact (after `vite build`)**

- No `*.map` files and no `sourceMappingURL`.
- No occurrence of the logo token value or `logo.dev`.
- No external hosts outside the allowlist.
- `index.html` contains no inline `<script>`.

**Rust tests (`src-tauri/tests/compliance_*.rs`)**

- `get_item_secret` rejects a field key not legal for the item kind.
- `get_item_secret`/`update_item` reject a vault not in the active User's `vault_ids` (once the
  ownership check lands).
- Every filesystem-facing command rejects a non-UUID id (traversal is structurally absent; assert it).
- Vault writes are atomic; `.OLD` is excluded from any sync path (ADR-0001 territory, asserted here
  as a guard).
- `zeroize` actually runs on lock.

**Renderer tests (vitest + jsdom, once the harness exists)**

- Reveal hides after 10 s under fake timers, and immediately on `blur`/`visibilitychange`.
- Hiding nulls the value; the next reveal refetches.
- Editor draft and original are null after unmount and route change.
- Clipboard compare-and-clear clears only when the content still equals the secret.
- Displayed text never contains the secret while masked.
- After a reveal, `localStorage`/`sessionStorage` are empty and the route has no secret.

### 4.2 What cannot be automated

OS-level and physical facts: per-user file permissions, installer signing, devtools absence in a
signed release, screen capture, OS clipboard history, and the memory-residency limitation. These are
`manual:` rows with a stated reason — not skipped rows.

### 4.3 The rule for the checklist

Every row in `audit-checklist.md` names either:

- a **test id** (e.g. `static:csp`, `rust:secret-kind`), or
- `manual: <reason>`.

A row with neither is incomplete. This is what makes "how it was measured" answerable rather than
rhetorical, and it is what lets a reader re-run the assessment instead of trusting it.

## 5. Anti-patterns

- **Asserting memory safety from source.** No test can prove a JS string was erased. Such claims are
  rejected; the limitation is documented instead.
- **Counting one control twice.** ASVS rows that a Tauri control discharges are linked, not counted
  as two passes (`research/audit-baseline.md` §6).
- **A checklist row that only says "reviewed".** It names a test or a manual reason.
- **A claim pinned to `main`.** Claims cite a commit; `main` moves.
- **Silent N/A.** Every not-applicable row carries the reason it does not bind.
