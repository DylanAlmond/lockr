# ADR-0002: Vault deletion leaves a tombstone

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** DylanAlmond

## Context

Vaults are single encrypted files named `<vault_id>.vault`. Deleting a vault used
to `remove_file` that path, so a mistaken delete — or a compromised renderer
calling `delete_vault` — destroyed the only copy of the encrypted data.

The renderer security audit (F-14) required deletion to move the file aside
rather than unlink it. That raises the questions ADR-0001 left open: what the
deleted file is called, what happens when the same vault is deleted twice, and
how a tombstone relates to the migration backup and the live vault set.

## Decision

### 1. Deletion renames, it does not unlink

`delete_vault` renames `<vault_id>.vault` to `<vault_id>.vault.deleted` beside
it in the vaults directory. The encrypted bytes are preserved exactly.

### 2. A tombstone is never overwritten

A repeated delete of the same vault — possible after an operator restores the
live file — uses a numbered suffix: `<vault_id>.1.vault.deleted`,
`<vault_id>.2.vault.deleted`, and so on. Every deletion remains recoverable.

### 3. Tombstones are outside the app's vault set

The `.deleted` suffix means `list_vault_ids` does not enumerate a tombstone and
`unlock_vault` never reads one. The app does not list, restore, or delete
tombstones; recovery is a manual, out-of-band rename back to `.vault`.

### 4. The migration backup is untouched

ADR-0001's `<vault_id>.vault.OLD` backup is still never read, rewritten, or
deleted by the app. Deleting a vault renames only the live `.vault` file, so a
pre-migration backup survives a deletion exactly as before.

## Consequences

- A deleted vault's encrypted file survives until the user removes it from
  disk. Disk usage grows with every deletion; accepted, because the files are
  small and silent cleanup would destroy the only recovery path.
- "Delete" is not irreversible at the storage layer, but it is irreversible in
  the product: there is no in-app restore, and the UI says so.
- Restoring a tombstone restores the vault file, not an unlocked session. The
  user still needs the master password.
- A stale `.vault.deleted` file is inert. It cannot be unlocked by the app and
  does not appear in any of its lists.
- If a profile write fails after the rename, the caller restores the file so
  the user's `vault_ids` and the live vault set cannot disagree.
