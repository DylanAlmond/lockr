# Glossary

The settled vocabulary of Lockr. Use these terms exactly as defined here — don't drift to synonyms. Terms in code use the exact identifiers noted.

## User

The owner of Vaults, who acts through one or more Devices. A User is not a Device, and not an Item. In code: `User`.

**Avoid:** "account" for the User — "account" names only the login item kind (see [Item](#item)).

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

## Device

A paired installation of Lockr that acts on a User's behalf. A User owns one or more Devices; every Device of the same User shares that User's identity and can decrypt the same Vaults. A Device is not a User — it is one place a User acts from.

## Pairing

The one-time act of admitting a new Device to a User, so that the Device shares the User's identity and may sync their Vaults. Pairing is the only grant of access: being on a network, or being discoverable, is never sufficient.

## Tombstone

What remains of a deleted Item or Vault, so the deletion can travel to the User's other Devices instead of the deleted thing reappearing on the next sync. A Tombstone carries no content and no secrets — only the identity of what was deleted, and enough information to know when every Device has seen the deletion.

## Duplicate

Two Items the User considers the same thing despite having been created separately, and so having different identities. The app may only *suggest* a Duplicate; it never acts on the suggestion. **Avoid:** "same item", "copy".

## Merge

Reconciling two copies of the same thing — the same Item or Vault identity — so that both Devices end up holding the same result. Merge is automatic wherever the copies do not contradict, and stops at a Conflict where they do. **Avoid:** "merge" for combining two *different* identities — that is **Combine**.

## Combine

The User's deliberate declaration that two distinct Items are one thing, retiring one identity in favour of the other. Never automatic, and never inferred from names. **Avoid:** Merge.

## Conflict

The state where two Devices independently changed the same field of the same Item, so that no ordering between the two changes exists. A Conflict is resolved by the User and blocks the sync that found it.

## Device Registry

The shared, signed record of every Device ever admitted to a User, and whether it has been revoked. A revoked Device stays in it permanently.

## Vault Registry

The shared record of which Vaults belong to a User, replacing the per-machine list of vault ids. It carries identities only — a Device that does not hold a Vault cannot even name it.

## Sync Set

The set of Vaults a particular Device participates in. It belongs to the Device rather than to the Vault, and removing a Vault from it never destroys anything.

## Revocation

Permanently barring a Device from syncing with the User's other Devices. Revocation cannot take back what the Device already holds, and no honest Device will sync with it again.
