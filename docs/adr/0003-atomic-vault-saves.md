# ADR-0003: Vault saves replace the file atomically

- **Status:** Accepted
- **Date:** 2026-11-27
- **Deciders:** DylanAlmond

## Context

A vault is a single encrypted file. Saving used `std::fs::write`, which truncates
the existing file before writing the new bytes. A crash, power loss or full disk
in that window left a truncated or empty vault, and the `.OLD` backup is only
written at migration time (ADR-0001), so it was not a recovery path for an
ordinary save.

## Decision

### 1. Write beside the vault, then rename over it

`save_vault` writes the encrypted payload to a sibling temp file,
`<vault_id>.vault.tmp`, then renames it over `<vault_id>.vault`. The temp file is
on the same filesystem, so the rename replaces the live file atomically.

### 2. Sync the temp file before the rename

The temp file is flushed with `sync_all` before the rename. On Unix the vaults
directory is fsynced afterwards on a best-effort basis so the rename itself
survives a crash; Windows has no directory fsync.

### 3. Failure leaves the old vault untouched

If the write, sync or rename fails, the temp file is removed and the live file
is never opened for writing. The in-memory vault is only marked clean after the
rename succeeds.

### 4. Temp files are not part of the vault set

A crash can leave a `.vault.tmp` behind. Its extension keeps it out of
`list_vault_ids`, and the next save truncates it rather than reading it.

## Consequences

- A crash between the write and the rename leaves either the old vault or the
  complete new one, never a truncated file.
- Saves carry one extra fsync. Accepted: vaults are small and saves are not on a
  hot path.
- The temp file holds the same encrypted payload as the vault, so it needs no
  separate protection.
- A stale temp file is inert. It is never unlocked, listed, or restored.
