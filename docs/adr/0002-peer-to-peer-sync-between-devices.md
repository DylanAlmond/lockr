# ADR-0002: Peer-to-peer sync between a User's Devices

- **Status:** Accepted
- **Date:** 2026-11-12
- **Deciders:** DylanAlmond

## Context

Lockr is single-machine by construction. A vault is one encrypted file holding one
writer's state: `<vault_id>.vault` is Base64 `salt(32) ‖ nonce(12) ‖ AES-GCM
ciphertext`, every save re-encrypts the whole thing, and nothing inside the file
is visible without the master password and the User's secret key. There is no
Device concept, no revision, no tombstone, no owner recorded in a vault, and no
network code of any kind — `Cargo.toml` carries no socket, HTTP, or discovery
dependency.

The README promises "no servers, no cloud, no network requests", and ADR-0001
assumes each vault file has exactly one writer on one machine. The user wants one
person's Vaults and identity on more than one machine, over the local network,
without a server. That requires deciding who is allowed in, how two divergent
copies reconcile, and — because a vault contributes nothing once encrypted — what
a Device may say about a vault it does not hold.

## Decision

### 1. One User, many Devices — no sharing between Users

Sync exists so a single User can use their own Vaults on several machines. Sharing
a Vault with *another* User is explicitly out of scope: it needs per-vault key
wrapping, an ACL, and revocation semantics over people rather than machines. A
vault has no vault-level key today — the vault key *is* the User's derived key —
so sharing would be a key-hierarchy rewrite, not a sync feature.

### 2. The master password is part of the shared identity

`vault_key = Argon2id(master_password ‖ secret_key, vault_salt)`. Two Devices
holding the same secret key but different master passwords derive different keys
and cannot read the same file. Pairing therefore requires entering the **same**
master password on the new Device. This is consistent with the code today — there
is no password-change feature — but it is a new constraint: changing the password
later is a change to the shared identity, and must re-encrypt every Vault on every
Device. The password itself must never cross the wire.

### 3. Pairing is the only grant of access

The new Device scans a single-use, short-lived 128-bit pairing secret rendered as a
QR code on the source Device (8-character typed fallback, ~60 s TTL, rate-limited).
Possession of that secret authenticates a TLS 1.3 mutual-auth channel with
ephemeral self-signed certificates, defeating a man-in-the-middle on the same
network. The human on the source Device then approves the joining Device by name
and platform, and only then is the user id, profile metadata, and 32-byte secret
key sent. The new Device re-wraps the secret key locally under the password typed
there.

Being on a network — or being discoverable — is never sufficient. A network
fingerprint may later *trigger* an automatic sync, but it never authorizes one.

Discovery advertises as little as it can: a service type and an opaque device id,
nothing about the User or their Vaults, and only while a sync is being sought.
A passive observer on the same network therefore learns that Lockr is present; it
learns neither who nor what. An unpaired Device is discoverable and still cannot
be synced with — discovery is a convenience for finding a Device that pairing has
already authorized, never a substitute for it.

### 4. Item identity is the UUID; duplicates are the user's problem

The merge engine never matches by name. Two account Items called "Amazon" with
different usernames are different accounts; worse, unifying them is *destructive*
rather than merely wrong, because field keys are fixed per kind and both would
collide on the single `password` key. One password would vanish with no undo.

After each sync the app computes a **suggestion** set — same kind, same normalised
display name, and for account Items the same username — and presents each as a
**Duplicate**. The user resolves it: keep both, keep either identity, or combine
them with an explicit pick for every colliding field, secrets included. Applying a
combine stamps the survivor with a fresh tick from the resolving Device, so the
next sync does not re-litigate it. Dismissals are stored in the vault, so one
Device's decision silences all of them.

### 5. Per-field merge with per-item vector clocks; conflicts block

Wall clocks cannot order events across Devices, and `updated_at` is display data
that already has no tie-break. Each Item therefore carries a version vector keyed
by `DeviceId`, and each field carries the Device and counter that last wrote it.

Merge is per field. Adding a tag on one Device and changing the password on the
other merges silently. Two Devices writing the *same* field concurrently is a
**Conflict**: merge stops, and the user resolves it before the sync completes.
Holding an unresolved conflict would require storing two values for one field key,
which breaks the field-key-per-secret invariant behind the Safe View Pattern, so
review-later is not an option.

Conflicts are surfaced by a blocking prompt *after* everything clean has been
merged. Because per-field merge removes disjoint-field false positives, this should
be rare.

### 6. Tombstones, and what deletion may and may not do

Deleted Items and Vaults leave a **Tombstone** — identity and causality only, no
content and no secrets. Without one, a deleted Item reappears from the peer on the
next sync.

- Item tombstones travel inside the vault; they are dropped once every paired
  Device has acknowledged a clock covering them. A Device that never returns
  leaves its tombstone in place indefinitely — acceptable, they are tiny.
- Vault deletion is the one remote action that cannot be undone: the receiving
  Device renames the file to `<vault_id>.vault.deleted` and leaves the purge to the
  user. Never a hard delete on a remote trigger.
- `<vault_id>.vault.OLD` never travels. ADR-0001 makes it a per-file, per-machine
  rollback path; the sync layer must exclude it explicitly.

### 7. Vaults may be combined, but only on explicit confirmation

Two Devices can independently create a vault with the same name: same intent,
different ids, different salts, different keys. Unlike Items, combining them is
reasonable — a vault is a grouping, not a credential — so the app offers it on
confirmation, never by name matching.

**Prerequisite:** `move_item` currently mints a fresh item UUID
(`vault_manager.rs:612`), which makes a move indistinguishable from delete+create
and would re-trigger the identity problem on every combine. `move_item` must
preserve item identity before vault combining ships.

### 8. A replicated device registry, with no authority device

Every Device keeps the same signed **Device Registry**: `DeviceId`, public key,
name and platform as announced, who admitted it, when, and whether revoked. It
syncs like any other shared state and merges as a set union by `DeviceId`, with one
asymmetric rule — **revocation always wins, forever**. Each entry is signed by the
admitting Device, so no Device can forge one attributed to another.

There is no primary or authority Device. The moment revocation is most needed is
"the Device I am holding is gone", and a single authority makes that the moment the
account cannot be recovered. Concurrent mutual revocation resolves to both revoked:
an honest deadlock, recovered by physically re-pairing from one of them.

`DeviceId` is minted by the installation itself on first use and is **admitted** by
pairing, never derived from hardware. A wiped and reinstalled Device returns with a
fresh identity and a fresh admission, because its local id is gone with its state;
the revoked entry stands forever, because what was revoked is the copy, not the
hardware. Minting on first use rather than at pairing is what lets a vault be
stamped with its origin Device before any Device has been paired.

A revoked Device can ignore the ban, and it keeps read access to everything it
already holds. The registry enforces "no honest Device will sync with this DeviceId
again" — nothing more, and the UI must say exactly that.

### 9. Shared registry, device-local profile

The profile file stops being "the user" and becomes "this Device's view of the
user". `encrypted_secret_key` is wrapped under *that Device's* password with its own
salt and nonce, so syncing it would overwrite one working wrapping with another for
no benefit: **device-local**.

`vault_ids` becomes a view over the shared **Vault Registry**, which is authoritative
on which vaults exist. Membership is ids only — a Device that does not hold a vault
cannot name it, because names live inside the ciphertext. A `.vault` file present on
disk but absent from the registry is **not adopted automatically**; adopting means
replicating its entire contents, so it takes an explicit confirmation.

User metadata (`name`, `color`, `icon`) is shared: two Devices showing different
names for the same User is a bug.

### 10. Sync scope belongs to the Device, not the Vault

Each Device has a **Sync Set**: which of the User's Vaults it participates in. It is
chosen at pairing from a pre-checked list (the consent that matters — admitting the
Device at all — already happened on the source machine), and remains editable.

Scoping is never a property of the Vault, because "this vault never leaves this
machine" cannot be kept once the vault already exists on another Device, and users
would read it as a deletion guarantee.

Three actions exist and must never be the same button:

1. **Stop syncing** — leaves the Sync Set. The local copy stays, keeps working, and
   drifts. Re-enabling warns that the divergence will merge, and applies any pending
   tombstone.
2. **Remove local copy** — destroys this Device's file only. Explicitly *not* a
   tombstone: it means "I don't want it here", not "it is gone".
3. **Delete vault** — the real one: tombstone, travels everywhere, becomes local
   trash on receivers.

### 11. Manual first; automatic later, gated by a trusted network

v1 syncs only when the user asks. Automatic sync makes the app a background
networked daemon, and the selling point of this app is being inert. The engine is
built so auto-sync is a trigger change rather than a rewrite: idempotent,
resumable, and callable without UI.

When auto-sync arrives it runs only on a network fingerprint the user has approved
once — SSID + BSSID, or gateway MAC + subnet for Ethernet — with any change
dropping back to manual. A spoofed SSID must at worst make the app *try*.

### 12. Transport and background decryption

Vault files are small and opaque, so Devices exchange **whole encrypted files** and
merge locally rather than building delta streaming. Merge must be a pure, symmetric
function of the two copies — commutative and associative — so both Devices compute
the same result without a further round-trip. Conflicts are the exception: one side
pauses, the user resolves, and the resolved vault is pushed.

The backend may decrypt vaults for a sync using the master password and secret key
already resident for the session, but only while the user is logged in and the app
is running — never a service that outlives the app — and with a visible indicator
while a transfer is in flight.

### 13. Atomic writes and external-change detection come first

`save_vault` writes straight onto the live path with no temp+rename, no fsync, and
no file locking (`vault_manager.rs:273-300`). With one writer that is merely risky;
with a merge engine and a possible background trigger it is data loss. **Atomic
write plus a "this file changed underneath me" check is a prerequisite PR, separate
from this feature.**

## Rejected options

- **Whole-vault last-writer-wins.** Far simpler and needs no tombstones, but it
  loses a clean concurrent edit (a tag on one Device, a password on the other) that
  per-field merge keeps, and pushes every disagreement into "pick a whole vault".
- **A primary Device as the authority for admitted Devices.** Simpler to reason
  about, but its loss locks out pairing *and* revocation at exactly the moment both
  are needed.
- **Network trust as authorization.** A spoofable local signal must never grant
  access; it may only trigger.
- **Name-based deduplication.** Silently destructive, as described in §4.
- **Reviewing conflicts after the fact.** Would require two values per field key.

## Schema v3 and migration

`CURRENT_SCHEMA_VERSION` becomes `3`. The shape is additive, so `ItemKind` and its
typed payload structs are untouched:

- `VaultItem` gains `clock: VersionVector` (a `DeviceId → u64` map) for its lifecycle
  — creation, move, deletion — and `field_clocks: BTreeMap<String, FieldVersion>`
  keyed by wire field name (`display_name`, `tags`, `username`, `password`, …), so
  each field's last writer is known without reshaping any kind into a string map.
- `SecretField` gains a `FieldVersion`. Secrets are fields; a concurrently changed
  password is a Conflict like any other.
- A deleted Item is retained with its clock and no payload or secrets: the tombstone
  *is* the Item, minus its content.
- The Vault Registry carries `VaultTombstone { vault_id, at }` entries.
- Duplicate dismissals live in the vault beside the Items.
- Adoption of an unregistered local `.vault` is recorded in the registry, never
  inferred from the directory.

**Migration.** v2 → v3 runs at unlock, whole-vault, exactly as ADR-0001 specifies:
byte-copy the encrypted file to `<vault_id>.vault.OLD`, transform, mark dirty. Each
Device stamps migrated Items with **its own** `DeviceId` at counter 1 — so two
independently migrated copies of the same vault are honestly concurrent, and the
first sync flags whatever actually differs. Stamping both with a shared synthetic
"legacy" id would make them look identical and silently pick one side.

Both sides of a sync must be on v3. A vault written by a v3 Device cannot be parsed
by a v2 one, and ADR-0001 already accepts that a rolled-back app cannot read a
migrated vault; the pairing handshake records the schema version and refuses to
exchange a vault the peer cannot read.

## Consequences

- **The README's "no network requests" claim must be amended** when this ships, to
  "offline by default; optional peer-to-peer sync on your local network". Until
  then the claim stays true because the feature does not exist.
- ADR-0001's single-writer assumption is superseded for synced vaults. `.OLD`
  remains local-only and per-machine.
- Vault files grow: per-field versions and per-item clocks are metadata that did not
  exist before. Clock keys also accumulate as Devices come and go, and are
  garbage-collected for revoked Devices once every remaining Device has acknowledged
  — safe, since a revoked Device can never write again.
- Password change becomes a multi-device re-encryption operation, and must propagate
  as part of this model rather than staying a local concern.
- Revocation does not revoke knowledge. A lost Device retains what it had; the
  honest remedy is that no Device will give it anything further.
- Implementation order, each independently testable: **(1)** atomic writes and
  external-change detection, **(2)** schema v3 and its migration, **(3)** registry
  and pairing, **(4)** replication and merge, **(5)** duplicate review, **(6)**
  auto-sync and trusted networks.
