# Glossary

The settled vocabulary of Lockr. Use these terms exactly as defined here — don't drift to synonyms. Terms in code use the exact identifiers noted.

## Vault

The top-level container a user owns. A vault holds items of every kind and the secrets belonging to those items. A user may own multiple vaults; each is stored as a separate encrypted file.

## Item

A single entry in a vault — a login, a card, a note, an identity. Items carry common metadata (display name, favourite, tags, icon, colour, timestamps) plus kind-specific data determined by their item kind. In code: `VaultItem`.

**Avoid:** "Account" for the generic thing a user stores in a vault — that is an **item**. "Account" names only the item *kind* (the login), surviving in kind-typed code (`AccountData`, `SafeItem::Account`, the `"account"` wire tag, `SecretInput`-keyed `password`) and as the legacy v1 wire format. Account items are created, updated, deleted, listed, and secret-fetched through the same generic item surface as every other kind.

## Item Kind

The category of an item, fixed at creation. The kinds are:

- **account** — a login: username, email, password
- **card** — a payment card
- **note** — freeform text
- **identity** — a personal identity: name, contact details, address

In code: the `ItemKind` enum, serialized on the wire as `"kind": "account" | "card" | "note" | "identity"`. A kind is fixed for an item's lifetime and determines which field keys the item accepts.

## Field Key

The name under which a single secret value is stored for an item — for example `password` on an account item, or `card_number`, `security_code`, `pin` on a card item. A field key maps to exactly one secret value; together a key and its value form a **secret field**. Secret fields live in the vault blob alongside the items they belong to, but are never sent to the UI except through the on-demand secret fetch (see [Safe View Pattern](#safe-view-pattern)).

## Schema Version

The version stamp on a vault's on-disk format (`schema_version`). It tells the app which shape the decrypted vault content has, so old files can be recognized and migrated instead of failing to parse. Files written before versioning existed omit the field and default to version 1.

## Migration Backup

The copy of a vault's encrypted file written immediately before a schema migration persists. Named `<vault_id>.vault.OLD`, sitting beside the vault file. It is the rollback path for a migration: the app itself never reads, rewrites, or deletes it — restoring it manually rolls the vault back.

## Safe View Pattern

The rule that secrets never leave the backend except through an explicit per-field fetch:

- List and detail results sent to the UI are **safe views** — models that structurally cannot carry secret values (`SafeVault`, `SafeItem`).
- A secret value reaches the UI only when requested, one field key at a time, via the on-demand secret fetch (`get_item_secret`). An unset field key fetches as null — 'no value exists' — while a failed fetch is an error; the two states are never conflated.
- Nothing deletes or exposes secrets as a side effect: listing, searching, and editing metadata never touch secret values.

The UI shows masked placeholders until a secret is explicitly revealed or copied.
