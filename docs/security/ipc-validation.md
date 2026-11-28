# IPC validation and authorisation

What crosses the renderer↔core boundary and what the core checks before acting.
Decision record for F-16, with the enforcement added by F-12, F-14 and F-17.

The rule in one line: **the renderer is untrusted; every command re-checks the
caller's authority and every field's shape in Rust.**

## 1. Authorisation

- Every vault/item command calls `ManagerState::require_owned_vault`, which
  asserts `vault_id ∈ active_user.vault_ids` before touching vault data.
- Scan commands (`get_all_items`, `get_item_by_id`, `get_unlocked_vaults`,
  `list_vault_ids`) are scoped to the active user's vaults by the **containing
  vault id**, not by an item's own `vault_id` field.
- Vault deletion additionally re-verifies the master password and uses the same
  login throttle; it moves the file to a tombstone rather than unlinking.
- `login_user` and `register_user` end the prior session first; logout, user
  deletion and the idle/absolute timeouts tear the session down.
- The renderer never supplies ids, timestamps or ownership: `update_item` takes
  named fields only, and `vault_ids` stays server-side.

## 2. Command categories

| Category | Commands | Checks |
|---|---|---|
| Unauthenticated | `get_users`, `get_user_by_id`, `register_user`, `login_user`, `get_password_strength` | ids parsed as UUIDs; registration password policy; login throttle |
| Session | `get_user`, `update_profile`, `logout`, `delete_user`, `session_activity`, `login_cooldown` | active user required where applicable; field bounds |
| Vault | `list_vault_ids`, `get_unlocked_vaults`, `get_vault_by_id`, `create_vault`, `unlock_vault`, `lock_vault`, `update_vault`, `delete_vault`, `flush_vault`, `flush_all`, `set_autosave`, `is_vault_dirty` | ownership; name/colour bounds; delete re-auth |
| Item | `add_item`, `update_item`, `delete_item`, `move_item`, `get_item_by_id`, `get_all_items`, `get_item_secret`, `copy_item_secret`, `update_item_secret`, `get_item_password_strength` | ownership; UUID parsing; field-key allowlist by kind; field bounds |

## 3. Field bounds

Character counts, not bytes. Applied before any mutation; an oversized value
returns a validation error and leaves the stored item unchanged.

| Field | Maximum |
|---|---|
| Vault / profile / item display name | 256 |
| Note title | 512 |
| Note body | 262,144 |
| Username | 256 |
| Email | 320 |
| Phone | 64 |
| Address line | 256 |
| City | 128 |
| Postal code | 32 |
| Country | 64 |
| Cardholder name | 256 |
| Card brand | 64 |
| Colour | 32 |
| Tag | 64 characters, 50 tags |
| Secret value | 4,096 |
| Icon | `data:image/png` / `data:image/webp`, or a relative name |

Ids are parsed as UUIDs before any path use; free text never reaches a path.
`move_item` checks ownership of both vaults.

## 4. Error surface

- Validation, authentication, authorisation and throttle errors carry a
  message the UI can show.
- I/O, serialization and not-found errors carry a generic message: the path,
  parser detail and identifier stay in `Debug`/`source`, never in the string
  that crosses IPC.
- No command prints user ids, paths or secrets to the terminal.
