# Renderer IPC surface inventory

Research for #64 (parent #63). Factual inventory only; no fixes proposed.

The renderer reaches Rust **only** through 30 `#[tauri::command]` functions registered in
`src-tauri/src/lib.rs:45-80`. No `.vue` file calls `invoke` directly — all renderer calls go
through `src/composables/useVault.ts`, `src/composables/useUser.ts`, and the dev-only
`src/seed.ts` (`rg "invoke\(" src`). Arguments are abbreviated below as **U** (user input),
**V** (vault/item-derived, i.e. a value the backend previously returned), **C** (constant in
`src/`), **R** (renderer-chosen; no UI path supplies it).

Working definitions used for the closing classification:

- **Presentation-only** — the call's effect is within what the current UI flow authorises.
- **Authorisation-bearing** — a compromised renderer calling it directly is not bounded by a
  user-intent gate: it crosses an identity/vault boundary, egresses plaintext secrets, or
  destroys state without re-authentication.

## 1. User / session commands

| Command | Call sites in `src/` | Args (origin) | Backend checks | Returns | Class |
|---|---|---|---|---|---|
| `register_user` | `useUser.ts:28` ← `RegisterView.vue:27`; `seed.ts:55` | name U, master_password U | none on content; stores mp/sk in RAM (`commands.rs:51-52`), creates "Personal" vault (`commands.rs:57-64`) | `Vec<SafeVault>` | presentation-only (unauthenticated by design) |
| `get_users` | `useUser.ts:15` ← `LoginView.vue:29` | — | none; reads every `*.json` in app data dir (`user_manager.rs:45-62`) | `Vec<User>` incl. `encrypted_secret_key`, `vault_ids` (`models.rs:86-98`) | presentation-only, exposes all profiles |
| `login_user` | `useUser.ts:46` ← `LoginView.vue:58` | id V (picker), password U | UUID parse (`commands.rs:132`), user registered (`user_manager.rs:111-113`), secret-key decrypt + 32-byte check (`user_manager.rs:121-126`); no lockout; **no lock of previously unlocked vaults** (`commands.rs:138-150`) | `Vec<SafeVault>` | authorisation-bearing (the auth decision) |
| `get_user` | `useUser.ts:73` (after register/login) | — | active user must exist (`commands.rs:78-82`); reads `active_path` (`user_manager.rs:157-165`) | `User` incl. `encrypted_secret_key` | presentation-only |
| `get_user_by_id` | **none** (registered only, `lib.rs:51`) | id R | UUID parse; `is_user_registered(id)` then reads `active_path` — `id` only gates existence, the returned record is the *active* user (`user_manager.rs:152-166`) | `User` incl. `encrypted_secret_key` | authorisation-bearing |
| `update_profile` | `useUser.ts:90` ← `SettingsView.vue:16`; `seed.ts:57` | U | active user only (`user_manager.rs:175-192`) | `()` | presentation-only |
| `logout` | `useUser.ts:61` ← `Sidebar.vue:19` | — | flushes then locks all vaults, zeroizes mp/sk (`commands.rs:159-171`) | `()` | presentation-only |
| `delete_user` | `useUser.ts:114` ← `SettingsView.vue:54` | — | only "a user is active"; **no password/re-auth argument**; deletes the user JSON (`user_manager.rs:224-240`), vault files untouched | `()` | **authorisation-bearing** |

## 2. Vault commands

| Command | Call sites in `src/` | Args (origin) | Backend checks | Returns | Class |
|---|---|---|---|---|---|
| `list_vault_ids` | `useVault.ts:38` (wrapper unused by components) | — | none; enumerates `*.vault` filenames (`vault_manager.rs:102-125`) | `Vec<String>` (all vault ids on disk) | presentation-only |
| `get_unlocked_vaults` | `useVault.ts:47`; `Sidebar.vue:55`; `New*Form.vue:62/75/95/125`; `MoveItemModal.vue:72`; `seed.ts:24` | — | none; returns **every** vault in RAM (`vault_manager.rs:128-133`) | `Vec<SafeVault>` | presentation-only (cross-session leak) |
| `get_vault_by_id` | `useVault.ts:56` ← `AccountDetails.vue:131`, `NoteDetails.vue:106`, `IdentityDetails.vue:107`, `CardDetails.vue:177` | vault_id V | UUID parse; must be unlocked (`vault_manager.rs:83-88`); no owner check | `SafeVault` | presentation-only |
| `create_vault` | `useVault.ts:68` ← `appStore.ts:101` ← `VaultModal.vue:46`; `seed.ts:20` | name/color U | requires cached mp/sk (`commands.rs:219-220`); appends id to active user (`user_manager.rs:196-208`); no name/color validation | `SafeVault` | presentation-only |
| `unlock_vault` | **none** (`lib.rs:60`) | vault_id R | UUID parse; file exists; decrypt with **cached** mp/sk (`commands.rs:260-265`, `vault_manager.rs:196-262`); no owner check | `SafeVault` | **authorisation-bearing** |
| `lock_vault` | **none** (`lib.rs:61`) | vault_id R / none | UUID parse; drops from RAM without flush (`commands.rs:271-278`, `vault_manager.rs:179-187`) | `()` | **authorisation-bearing** (silent unsaved-data loss) |
| `update_vault` | `useVault.ts:80` ← `appStore.ts:114` ← `VaultModal.vue:44` | vault_id V, name/color U | UUID parse + unlocked; no owner check (`vault_manager.rs:343-361`) | `()` | presentation-only |
| `delete_vault` | `useVault.ts:94` ← `appStore.ts:124` ← `Sidebar.vue:70` | vault_id V | UUID parse; removes id from active user's list (`user_manager.rs:211-221`, silent no-op if absent); deletes the file — **no owner check, no unlock requirement** (`vault_manager.rs:364-374`) | `()` | **authorisation-bearing** |

## 3. Item commands

| Command | Call sites in `src/` | Args (origin) | Backend checks | Returns | Class |
|---|---|---|---|---|---|
| `add_item` | `useVault.ts:124` ← `appStore.ts:46` ← `NewItemModal.vue:43/51/59/67`; `seed.ts:36` | vault_id V, metadata U, kind data U, secrets U | UUID; vault unlocked (`vault_manager.rs:470`); kind via `NewItemData` enum (`models.rs:209-216`); secret keys ⊆ `allowed_secret_keys` for kind (`vault_manager.rs:425-453`, `models.rs:32-42`); card expiry 1–12 (`vault_manager.rs:413-418`); no owner check; no length caps | `SafeItem` | presentation-only |
| `update_item` | `useVault.ts:175` ← `appStore.ts:57` ← `AccountEdit.vue:168`, `NoteEdit.vue:63`, `CardEdit.vue:185`, `IdentityEdit.vue:85`; also favourite toggles `*Details.vue:36/59/110/37` | vault_id/item_id V, fields U | UUID ×2; item exists **in that vault** (`vault_manager.rs:513-517`); kind change rejected (`vault_manager.rs:548-552`); **card expiry not re-validated** (contrast `413-418`); no owner check | `SafeItem` | presentation-only |
| `delete_item` | `useVault.ts:193` ← `appStore.ts:69` ← `*Details.vue:55/78/125/56` | vault_id/item_id V | UUID ×2; item exists in vault (`vault_manager.rs:380-383`); drops item + secret entry; no owner check | `()` | presentation-only |
| `move_item` | `useVault.ts:207` ← `appStore.ts:88` ← `MoveItemModal.vue:50` | all V (target vault from unlocked list) | UUID ×3; distinct vaults (`vault_manager.rs:586-590`); **both** must be unlocked (`593-594`); item in source (`599-603`); target owner never checked | `SafeItem` | presentation-only (flag F9) |
| `get_item_by_id` | `useVault.ts:106` ← `appStore.ts:39` ← `ItemPanel.vue:33` | item_id V | UUID; item must exist in **any** unlocked vault (`vault_manager.rs:568-576`); no vault argument at all | `SafeItem` (no secret values) | presentation-only |
| `get_all_items` | `useVault.ts:115` ← `ItemList.vue:170` | filter: vault_id V (route), search U | UUID via `get_vault` when filtered; with `vault_id: None` scans **all** unlocked vaults (`vault_manager.rs:640-644`); no owner check | `Vec<SafeItem>` (notes bodies, identity PII, card last4, `secrets_present`) | presentation-only |
| `get_item_secret` | `useVault.ts:145`; `AccountDetails.vue:40`, `AccountEdit.vue:64`, `CardDetails.vue:85`, `CardEdit.vue:83` | vault/item V; field_key C (`util/secrets.ts`) | UUID ×2; vault unlocked; item in vault (`vault_manager.rs:704-708`); kind has secrets (`711-715`); key legal for kind (`716-720`); **no owner check** | `Option<String>` — **plaintext secret** (`vault_manager.rs:725`) | **authorisation-bearing** (secret egress) |
| `update_item_secret` | `useVault.ts:161`; `AccountEdit.vue:189`, `CardEdit.vue:208` | vault/item V, key C, value U | same kind/key/item checks (`vault_manager.rs:738-764`); no owner check; no length cap | `()` | presentation-only |
| `get_item_password_strength` | `useVault.ts:221` ← `AccountDetails.vue:132` | V | delegates to `get_item_secret(vault, item, "password")` (`vault_manager.rs:803-815`) | `Entropy` (score/guesses, not the secret) | presentation-only |

## 4. Utility commands

| Command | Call sites in `src/` | Args (origin) | Backend checks | Returns | Class |
|---|---|---|---|---|---|
| `get_password_strength` | `useVault.ts:230` ← `AccountEdit.vue:88`, `NewAccountForm.vue:89` | plaintext password U | **none** — stateless zxcvbn (`commands.rs:329-332`) | `Entropy` | presentation-only |
| `flush_vault` | `useVault.ts:239` (wrapper unused by components) | vault_id R | UUID; no-op if not dirty/locked (`vault_manager.rs:304-315`) | `()` | presentation-only |
| `flush_all` | `useVault.ts:249` (unused) | — | none; writes every dirty unlocked vault (`vault_manager.rs:318-331`) | `()` | presentation-only |
| `set_autosave` | `useVault.ts:259` (unused) | enabled R | none; global flag (`vault_manager.rs:62-64`) — off means later mutations are RAM-only until flush | `()` | presentation-only (integrity flag) |
| `is_vault_dirty` | `useVault.ts:269` (unused) | vault_id R | UUID; `false` for locked (`vault_manager.rs:334-339`) | `bool` | presentation-only |

## 5. Flags — missing or absent checks

Each flag is a factual gap; reachability is stated only where the code shows it.

1. **No vault-ownership check exists anywhere.** `vault_ids` is read only in the login
   unlock loop (`commands.rs:148`); no command compares a requested `vault_id` against
   `active_user_id`. Every vault command gates on "vault is in the RAM map"
   (`vault_manager.rs:83-99`), not "vault belongs to the active user". Consequence: any
   unlocked vault is fully readable/writable (`commands.rs:202-206`, `234-244`, `375-390`).
2. **Cross-session vault retention.** `login_user` and `register_user` overwrite the cached
   mp/sk without locking what is already unlocked (`commands.rs:51-52`, `138-150`), while
   `get_unlocked_vaults` (`vault_manager.rs:128-133`) and `get_all_items` with
   `vault_id: None` (`vault_manager.rs:640-644`) return **all** unlocked vaults. A renderer
   that calls `login_user` directly (bypassing the UI's `logout`) can thus still see the
   previous session's vaults. Only `logout` clears them (`commands.rs:164`).
3. **Plaintext secret egress is a single unchecked call.** `get_item_secret` validates the
   item and field key but not the caller, user, or UI state (`commands.rs:467-482`,
   `vault_manager.rs:696-726`). Secrets sit inside the decrypted vault JSON
   (`vault_manager.rs:284-287`), so "vault unlocked" is the whole authorisation model.
4. **`delete_user` destroys the profile with no re-authentication and no argument to
   confirm intent** (`commands.rs:177-190`; `user_manager.rs:224-240`). Vault files are left
   on disk, and their ids remain enumerable through `list_vault_ids`
   (`vault_manager.rs:102-125`).
5. **`delete_vault` deletes a file for an arbitrary id, locked or not, without ownership or
   confirmation** (`commands.rs:297-311`; `vault_manager.rs:364-374`). Removing the id from
   the user's list silently succeeds for ids that were never in it
   (`user_manager.rs:218`).
6. **Two registered commands have no renderer call site**: `unlock_vault`
   (`lib.rs:60`) and `lock_vault` (`lib.rs:61`); likewise `get_user_by_id` (`lib.rs:51`).
   They are still invokable. `unlock_vault` spends the cached credentials on any vault id
   (`commands.rs:260-265`); `lock_vault` discards unsaved changes because it does not flush
   first (`commands.rs:277` vs `commands.rs:162`).
7. **`get_user_by_id` ignores its `id` for data selection.** It parses and existence-checks
   the id, then returns whatever `active_path` points at (`user_manager.rs:152-166`). So it
   cannot read another user — but it also cannot do what its name says, and the id argument
   acts only as a registered/not-registered oracle.
8. **All user profiles and their `encrypted_secret_key` blobs are exposed to the renderer**
   via `get_users`, `get_user`, `get_user_by_id` (`user_manager.rs:42-65`, `152-166`;
   `models.rs:86-98`). Not plaintext, but it is offline-guessing material for the master
   password; the blob is the only thing standing between a renderer compromise and a full
   offline attack.
9. **`update_item` skips the card-expiry validation that `add_item` performs**
   (`vault_manager.rs:413-418` vs `535-547`).
10. **No login attempt limiting or lockout** in `login_user` or `user_manager.login`
    (`commands.rs:117-153`, `user_manager.rs:110-132`); per-vault unlock failures are
    swallowed (`let _ =`, `commands.rs:149`), so login "succeeds" with a partial vault set.
11. **`move_item` accepts any unlocked target vault** (`vault_manager.rs:586-594`); nothing
    requires the target to be in the active user's `vault_ids`.
12. **No password policy on any path.** `register_user` accepts any master password
    (`commands.rs:43-49`); the strength meter is a frontend-only convenience
    (`NewAccountForm.vue:89`) and `RegisterView.vue` never calls it.

**Path traversal:** structurally absent. Every filesystem-touching id is parsed to `Uuid`
before use (`commands.rs:236,251,274,289,300,320-321,339,368,384,405-406,417-418,434-436,447,475-476,493-494`)
and paths are built from the formatted UUID (`vault_manager.rs:67-69`); `list_vault_ids`
only reads directory entries (`vault_manager.rs:109-122`). Free-text fields (`name`,
`color`, `icon`, `display_name`) never reach a path.

**Safe-view vs plaintext:** only `get_item_secret` returns a secret value. Everything else
returns `SafeVault`/`SafeItem`/`User`/`Entropy`/`bool`/`()`. `SafeItem` excludes passwords
and card secrets, exposing only `card_number_last4` and `secrets_present`
(`models.rs:506-530`, `605-644`). Note that `SafeItem` still carries note bodies and full
identity PII (`models.rs:459-501`), and `User` carries the encrypted secret key
(`models.rs:86-98`).

**Adjacent threat-model inputs (not IPC commands):** the window has **no CSP**
(`src-tauri/tauri.conf.json:27`), so any renderer injection yields full access to this
surface; capabilities grant only `core:default` and `opener:default`
(`src-tauri/capabilities/default.json:5-12`), so no `fs`/`shell`/`http` plugin is reachable —
these 30 commands are the entire native surface.

## Open questions

- Is multi-user-per-machine a supported model? `get_users` lists every profile
  (`user_manager.rs:42-65`) and `vault_ids` looks like an intended ACL, but nothing enforces
  it. Whether that is deliberate is not decidable from these files.
- Is `unlock_vault`/`lock_vault` reserved for a future per-vault unlock UI, or dead surface?
- Is leaving vault ciphertext behind after `delete_user` intentional, or an oversight?
- Is the missing re-auth on `delete_user`/`delete_vault` accepted because the renderer is
  trusted, or does #63's threat model treat the renderer as compromisable?
- Is `lock_vault`'s lack of flush-before-lock intentional (loss of unsaved changes when
  autosave is off)?
- Where `active_path`/`active_user_id` are meant to be asserted is unclear; no code in
  `commands.rs` or `vault_manager.rs` consults them outside the login loop.
