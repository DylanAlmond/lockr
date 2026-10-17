# ADR-0001: Schema versioning with backup-on-migrate

- **Status:** Accepted
- **Date:** 2026-10-16
- **Deciders:** DylanAlmond

## Context

Vaults are stored as single encrypted files. The earliest format was an accounts-only shape: every entry was an account with an `AccountSecret` holding a single `password`. The versioned model generalizes entries into items of any kind (account, card, note, identity) whose secrets live in per-field secret entries keyed by field key — a fundamentally different shape.

When the app encounters a vault written by the old format, it has to turn it into the new one. That raises four questions:

1. How does the app know which format a file is in?
2. When does migration run?
3. What protects the user's data if the migration is buggy or unwanted?
4. What happens to vaults written by the new format if the user rolls the app back?

## Decision

### 1. An explicit schema version field, not parse-fallback

The vault carries a `schema_version` field (`CURRENT_SCHEMA_VERSION: u32 = 2`). Vault files written before versioning existed omit the field, so serde defaults it to `1` — pre-versioning files are v1 by construction.

Parsing still tries the current format first and falls back to the v1 accounts-only format, but the version field is the authority: a v2-shaped file with an outdated stamp is re-stamped and migrated, and the stamp tells us which migrations the content has been through. Parse-fallback alone can't distinguish "old format" from "corrupt current format" once more than two formats exist.

### 2. Full migration at unlock, while the model is small

Migration happens inside `unlock_vault`, in one step: the entire decrypted vault is transformed to the current format and marked dirty, so the next save persists it whole. No incremental or per-field migrations, no version-by-version stepping.

This is a deliberate trade for simplicity: with a small model and one historical version, a single whole-vault transform is easy to test (`test_migrates_legacy_vault_on_unlock`) and impossible to leave half-applied. If many schema versions accumulate, this decision should be revisited.

### 3. Backup before migrate: `<vault_id>.vault.OLD`

Before any save can persist the transformed format, `unlock_vault` copies the still-encrypted vault file byte-for-byte to a backup named `<vault_id>.vault.OLD`, beside the vault in the vaults directory.

The convention:

- **Written** only at migration time — plain unlocks of an already-current vault never touch the backup (pinned by `test_migrates_legacy_vault_on_unlock`, which asserts a v2 re-unlock leaves the backup byte-identical).
- **Overwritten** by any later migration of the same vault; there is exactly one backup per vault, holding the last pre-migration state.
- **Never deleted by the app.** Not on unlock, lock, save, or vault deletion — `delete_vault` removes only the `.vault` file. The backup lives until the user deletes it.
- **Unread by the app.** Restoring it — replacing the `.vault` file — is a manual, user-driven rollback.

A file that parses as neither format is corrupt: unlock fails with the serialization error, the original file is untouched, and no backup is written.

### 4. The backup is the rollback path — accepted trade-off

A pre-versioning app cannot read a migrated vault: once the versioned format is saved, the old app has no code path for items, item kinds, or per-field secrets. We accept this rather than blocking saves on migrated vaults or carrying dual-format write support.

The migration backup (§3) is the mitigation: it holds the exact pre-migration bytes, and restoring it — replacing the `.vault` file — is the user-driven rollback.

## Consequences

- Every vault file from v2 onward is self-describing; v1 files are recognizable by their missing stamp and accounts-only shape.
- Migration is atomic in effect: either the old file is still on disk, or the next save persists the fully migrated format — never an in-between.
- Users get a free, app-created rollback point at exactly the moment the format changes, with no user action required.
- Exactly one backup per vault: migrating twice in a row loses the first backup. Acceptable while there is one historical version; revisit if version chains grow.
- Backups accumulate in the vaults directory until users delete them — accepted, since they are small (one encrypted file each) and silently deleting them would destroy the rollback path.
- Stale copies of a migrated vault (OS file sync, manual copies) do not carry the backup with them; the backup is per-file and per-machine, not part of the vault itself.
