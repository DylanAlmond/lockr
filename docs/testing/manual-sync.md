# Manual sync test script

This is the human-verified companion to the automated two-Device scenarios in
`src-tauri/src/sync.rs`. It covers the paths that automated tests cannot reach:
the real TLS transport, mDNS on a physical network, the Vue screens, and the
actions a person takes when something goes wrong.

Run it on **two separate machines** on the same local network. A row marked
**TWO MACHINES** cannot be completed on one machine: two Lockr installations on
one host share an app-data directory and a Device identity.

Origin: ticket #45 asked for this script, but the tracker now contains only the
note that the ticket was completed. This version was reconstructed against the
current build (the Devices screen, pairing over the transport, the post-pairing
transfer, conflict reveal and the three removal actions) and is the copy that
should be maintained from now on. When a step changes, change it here in the
same commit as the behaviour.

## Before you start

Record these once per run:

| Item | Device A | Device B |
| --- | --- | --- |
| Hostname | | |
| OS and version | | |
| Lockr commit (`git rev-parse HEAD`) | | |
| Build (`yarn tauri build` or `yarn tauri dev`) | | |
| App version from Settings | | |

Both Devices must run a build from the **same commit**. A schema mismatch is
reported deliberately, not worked around.

Network requirements:

- Both machines on the same subnet, with client isolation / AP isolation off.
- mDNS allowed (UDP 5353) and Lockr allowed through the host firewall on
  private networks.
- No VPN or proxy that hides multicast from the other machine.
- Both machines awake, with Lockr open on the **Devices** screen. Discovery
  stops when that screen closes.

App-data locations, used by the removal checks:

| OS | Directory |
| --- | --- |
| Windows | `%APPDATA%\com.karkand.lockr` |
| macOS | `~/Library/Application Support/com.karkand.lockr` |
| Linux | `~/.local/share/com.karkand.lockr` |

Vault files live under `<app-data>/vaults/<vault-id>.vault`. A migration backup
is `<vault-id>.vault.OLD`; a Vault deleted everywhere becomes
`<vault-id>.vault.deleted`. Neither suffix is adopted or synced.

For the cleanup steps, a fresh Device B means: quit Lockr, delete the app-data
directory above, reinstall or relaunch. That removes its Device identity and
local User. Do not do this to a Device you still need.

## Known gaps in this build

These are observations from the code at the time of writing. They are listed so
a run records what actually happened instead of quietly passing a step that was
never reachable.

1. **The join entry point is on the sign-in screen, not the Devices screen.**
   A fresh Device pairs through **Join a Device instead of creating a User**,
   which is only offered while no local User exists. A Device that already has
   a User says so on that screen and cannot join; add Devices from the Devices
   screen with **Pair a new Device** instead. Builds before #60 had no such
   entry point and M3 could not run.
2. **A conflict with no local side cannot be constructed by hand.** It needs a
   registry/merge state where both sides were written by other Devices. The
   automated suite covers that branch; the manual script marks it `SKIP
   (automated)`, not `PASS`.

## Result sheet

| Module | What it proves | Machines | Result |
| --- | --- | --- | --- |
| M0 | Build and baseline | One, repeated | |
| M1 | Discovery without access | TWO MACHINES | |
| M2 | Pairing over the real transport (source side) | TWO MACHINES | |
| M3 | Joining side and approval | TWO MACHINES | |
| M4 | Post-pairing Vault transfer | TWO MACHINES | |
| M5 | Cancel a running sync | TWO MACHINES | |
| M6 | Conflict reveal and resolution | TWO MACHINES | |
| M7 | The three removal actions | TWO MACHINES | |
| M8 | Revocation and Sync Set edges | TWO MACHINES | |

Use `PASS`, `FAIL`, `BLOCKED` or `SKIP`. Attach a screenshot for every `FAIL`.

---

## M0 — Build and baseline

**ONE MACHINE**, repeated on both.

1. From a clean checkout of the agreed commit:

   ```sh
   yarn install
   cargo test --manifest-path src-tauri/Cargo.toml --lib
   yarn build
   ```

   The baseline at the time of writing is **289 backend tests passing** and a
   green `yarn build`. A different count is not automatically a failure, but
   every test must pass; record the count.
2. Start Lockr on Device A. Register a User, then create five Vaults:
   **Work** and **Personal**, plus **Archive**, **Scratch** and **Legacy** for
   the removal checks.
3. In Work, create items of each kind: an account with a known username,
   email and password; a card; a note; an identity. In Personal, create at
   least one account. Put one known account item in each of Archive, Scratch
   and Legacy so their contents can be checked.
4. Record the known secret values somewhere outside Lockr. You will compare
   them after the transfer.
5. Open **Devices** from the sidebar. Note who is listed under Paired Devices
   (only this Device) and that the other machine is not paired.

Expected: the Devices screen loads, discovery starts, and no error banner is
shown. `This Device's identity is damaged` must not appear.

Result: `______`

## M1 — Discovery without access

**TWO MACHINES**

1. Leave Device A on Devices; open Device B on Devices (register a local User
   for this step only; B does not need Vaults).
2. Wait up to 5 seconds, or press **Refresh** under **On this network**.
3. Each Device should show the other under **On this network**, tagged
   **Unknown**, with an address. Neither should appear under Paired Devices.
4. On a discovered unknown peer, the action must be **Pair…**, not **Sync now**.
   Seeing a Device is not access.

Expected: both machines see each other; no sync is possible before pairing.
Record the peer label and address on each side.

Result: `______`

> If B registered a User for this step, delete its app data before M2/M3 so it
> is a fresh Device. A Device with a local User is not a valid joining Device.

## M2 — Pairing over the real transport (source side)

**TWO MACHINES**

1. On Device A (the Device that already has the Vaults), open **Pair a new
   Device**.
2. Press **Start pairing**. This Device is the source because it is signed in
   with your User; the new Device must have no User yet.
3. Verify an offer appears with:
   - a short typed code in `ABCD-FGHJ` form;
   - this Device's **Device id**, shown next to the code and copyable with
     **Copy code and Device id**;
   - a countdown starting near 60 seconds;
   - a collapsible **QR payload** beginning `lockr-pair:v1:` and a **Copy
     payload** button.
4. Verify the offer expires after about a minute without accepting anything.
   The UI must say the code expired and offer **Start over**. Start a fresh
   offer.
5. With the fresh offer showing, repeat the join from M3 on Device B using the
   typed code and Device id (or the **QR payload**). On Device A, while waiting,
   verify the text says it is listening over the secure pairing channel; it must
   not claim the Device is already approved.
6. When Device B connects, verify the approval card appears on A with B's
   **name**, **platform** and **device id**. Confirm these match the machine
   running B. A Device that merely answers on the network must not appear here
   without the pairing secret.
7. Verify the **Vaults this Device will carry** list is pre-checked. Leave all
   five checked for this run so the later modules have the Vaults they need.
   (Optional extra: uncheck one, finish the transfer, and verify it does not
   arrive; then enable it from B's Sync Set editor and verify the next sync
   fetches it.)
8. Press **Approve and send access**. Verify the success panel says the new
   Device holds the User id, shared profile metadata and secret key, and that
   the master password was never sent. A copy of the master password must not
   appear anywhere on A.
9. On A, verify B now appears under Paired Devices with **Never synced with
   this Device** (or a sync time after M4).

Expected: the approval is over the live TLS channel; the typed/QR secret is
single-use; nothing is admitted before a human approves. Record A's typed code
and the approval card values.

Result: `______`

## M3 — Joining side and approval

**TWO MACHINES**

The join entry point is on the **sign-in screen** of a Device with no local
User. The Devices screen has no join action, because a signed-in Device already
belongs to a User and the backend refuses to join in that state.

1. Start Lockr on a **fresh** Device B with no local User. On the sign-in
   screen choose **Join a Device instead of creating a User**.
2. Verify the screen explains the prerequisites: A must be signed in and show
   a pairing code and Device id; the master password is typed on B.
3. Type the **Device id** and **pairing code** from A's offer card, then press
   **Continue**. The **Paste the QR payload** option must also work; use one
   path for this run and record which.
4. Verify B waits for the source Device and says a human on the source must
   approve it. B must not hold a usable identity yet.
5. After A approves in M2, verify B shows **Approved by the source Device**,
   the User name, and a master-password field. The master password is typed on
   B only.
6. Enter a **wrong** master password first. Expected: pairing does not
   complete; no Vaults appear; B can retry. Then enter the correct password and
   press **Finish pairing**.
7. Verify B does **not** ask for the password to be sent anywhere and that the
   app proceeds directly to the post-pairing transfer (M4).
8. After M4, sign out on B. Verify the sign-in screen now says this Device
   already belongs to a User and does not offer **Join a Device**. That refusal
   is stated before a join can start, not reported as an error afterwards.

Expected: the joining Device becomes the same User; the transfer is verified
against the Device B admitted; a wrong password cannot complete it; a Device
that already has a User cannot begin.

Result: `______`

## M4 — Post-pairing Vault transfer

**TWO MACHINES**

1. Immediately after **Finish pairing** on B, verify the panel changes to
   **Fetching your Vaults** and then either:
   - `Your Vaults are on this Device and ready to open (N Vaults)`, or
   - `Pairing succeeded, but the first Vault sync did not finish` with a
     **Retry Vault sync** button.
   The transfer is automatic; the User should not have to find Sync now first.
2. Verify all five Vaults arrived (or exactly the checked subset if you ran the
   optional uncheck in M2). The count in the panel must match what A approved.
3. Open Work on B and verify the account, card, note and identity are present
   with the same metadata (name, tags, favourite).
4. Verify secret fields are masked. Reveal the account password explicitly and
   compare it with the recorded value; then hide it. Repeat for one card field.
   Secrets must not be present in the list before the reveal.
5. On A, press **Sync now** for B. Verify it completes with no conflicts and B's
   last-sync time updates. No new Vaults or items should appear.
6. On A, open the Sync Set editor and confirm all five Vaults are marked
   **Syncing**.
7. Optional: if the first transfer is interrupted, verify **Retry Vault sync**
   succeeds and no half-written Vault is listed.

Expected: pairing transfers identity and metadata; the Vault bytes arrive
through the ordinary sync engine; the checked Sync Set is honoured.

Result: `______`

## M5 — Cancel a running sync

**TWO MACHINES**

Make the sync long enough to cancel: put a large note (a few MB) in a Vault, or
create several hundred items. Do not change code or throttle the network.

1. On A, start **Sync now** with B.
2. While the panel shows progress (**Sending…**, **Receiving…**, or the
   exchanging step), press **Cancel sync**.
3. Verify the panel reports **Sync cancelled** within about a second, not after
   the 30-second frame timeout. The advice must say that any Vault which finished
   transferring is safe to open and everything else was left as it was.
4. Verify both Devices still open their Vaults and no file is corrupt. A partial
   Vault must not be installed or listed.
5. Verify leaving the Devices screen while the sync is running is blocked with
   "A sync is running…", and that the block clears after cancellation.
6. Repeat the cancel during the discovery wait (the other Device closed). The
   same cancelled result should appear, without waiting for the discovery
   timeout.
7. Start a fresh sync and let it finish. Verify cancellation left nothing behind
   that prevents a clean retry.

Expected: cancel is observed at the next short read slice; no half-written Vault
survives; a retry works.

Result: `______`

## M6 — Conflict reveal and resolution

**TWO MACHINES**

Both Devices need the same Vault (Work) with the same item. Finish any pending
sync first.

### M6a — Non-secret conflict

1. On A, edit the item's **display name** to `Conflict from A`. Lock it.
2. On B, edit the same item's display name to `Conflict from B`. Lock it.
3. From A, sync with B. Verify the conflict prompt opens and says the sync stays
   open until every field is resolved.
4. Verify both values are shown, that the local side is labelled **This Device**
   / **This copy**, and that the other side names B.
5. Choose one side. Verify the prompt closes when no conflicts remain and the
   chosen value is present on both Devices after the next sync.

### M6b — Secret conflict and reveal

1. On both Devices, change the **password** of the same account item to two
   different known values. Lock both.
2. Sync from A. Verify the conflict prompt shows the secret field without either
   value: both sides say **Value hidden**. The value must not be in the conflict
   payload or in the rendered page before reveal.
3. Press **Reveal value** on one side only. Verify only that side reveals, the
   other stays hidden, and the button becomes **Hide value**.
4. Press **Hide value**; both sides are masked again.
5. Move to another Conflict with **Next** (or resolve this one), then back.
   Verify the revealed value is re-masked when the current Conflict changes; a
   secret must not follow you between conflicts.
6. Choose a side **without revealing it**. Verify the chosen password is now the
   item's password on the resolving Device.
7. Resolve the remaining conflicts and verify the sync completes. Sync both
   directions and confirm the password matches on both Devices.

### M6c — Leave unresolved

1. Create another conflict. Press **Leave unresolved**.
2. Verify navigation is unblocked, clean merges are kept, this Device's value is
   kept, and the other side was not applied. The sync is not reported as
   complete.
3. Sync again. The conflict should be surfaced again rather than silently
   disappearing.

### M6d — No local side

`SKIP (automated)` unless you have crafted state where both sides came from
other Devices. The prompt must say "This conflict arrived from other Devices;
sync again to refresh" and offer **Sync again**, never a blind local/remote
choice.

Result: `______`

## M7 — The three removal actions

**TWO MACHINES**

These actions are deliberately different. Run them on separate spare Vaults so a
mistake does not cost data: **Archive** for stopping, **Scratch** for removing a
local copy, and **Legacy** for deleting everywhere. Both Devices hold all three
after M4.

### M7a — Stop syncing Archive (keeps the local copy)

1. On A, open the Sync Set editor and find Archive under **Vault removal
   actions**. Press **Stop syncing** and confirm.
2. Verify A still lists and opens Archive with all its items. Its badge says
   **Not syncing**. The action must not delete anything.
3. Edit an item in Archive on A. Sync A with B.
4. Verify B's Archive did not receive the edit; the copies have drifted. This is
   the intended consequence.
5. Re-enable Archive in the Sync Set and save. Sync again. Verify the divergence
   merges (or produces a Conflict, per M6) instead of one copy silently winning.

### M7b — Remove Scratch from this Device (no deletion travels)

1. On A, press **Remove local copy** for Scratch and confirm by typing the Vault
   name if prompted.
2. Verify Scratch disappears from A's Vault list and cannot be opened there.
3. In `<app-data>/vaults` on A, verify `<id>.vault` is gone and **no**
   `<id>.vault.deleted` exists. This action is not a deletion and leaves no trash.
4. On B, verify Scratch is still present and openable. No deletion arrived.
5. Sync A with B. Verify Scratch is **not** reinstalled on A, because removing
   the local copy also removed it from A's Sync Set.
6. Verify no Tombstone was recorded: B still lists Scratch as a live Vault after
   the sync.

### M7c — Delete Legacy everywhere (the destructive one)

1. On B (a Device that still has Legacy), press **Delete everywhere** and
   confirm by typing the Vault name.
2. Verify Legacy disappears from B's Vault list. In B's app data, verify
   `<id>.vault` has become `<id>.vault.deleted` and still exists. Do not empty
   the trash as part of this test.
3. On A, which still holds a live Legacy, sync with B. Verify Legacy disappears
   from A's Vault list and that A's `<id>.vault` has become
   `<id>.vault.deleted`. The deletion must travel as a Tombstone; A must not
   reinstall the Vault.
4. Verify every Device that syncs afterwards leaves it deleted. Deleting cannot
   be undone by syncing. Re-adding a file with the same id is not a supported
   resurrection path.
5. Verify the confirmation copy said other Devices keep a local trash copy and
   that this is the destructive action.

Result: `______`

## M8 — Revocation and Sync Set edges

**TWO MACHINES**

1. On A, press **Revoke** for B. Type B's name to confirm. Verify the copy warns
   that revocation is permanent, travels on the next sync, and does not take back
   what B already holds.
2. Verify B's card on A shows **Revoked** and no **Sync now** or **Revoke**
   button.
3. On B, try to sync with A. Verify B reports that this Device has been revoked
   (or that A is revoked), with no data exchange.
4. On A, sync with another admitted Device if one exists. Verify the revocation
   travels and that Device also refuses B.
5. Verify a revoked Device still appears in the registry permanently; it is not
   deleted from the list.
6. Re-pairing B as a **new** Device is the documented recovery path: delete
   B's app data so it has no User, start it, choose **Join a Device instead of
   creating a User**, and pair from A. The old revoked entry remains.
7. Finally, leave the Devices screen on both machines. Verify discovery stops
   (the other Device disappears from **On this network** after the next refresh
   or timeout) and no sync is possible without reopening the screen.

Result: `______`

## Cleanup

1. Quit both apps.
2. If this was a scratch run, delete the app-data directory on both machines.
3. Do not delete `.vault.deleted` or `.vault.OLD` files unless you are sure they
   are not needed; they are the recovery paths.
4. Record the results table and any screenshots with the run's commit hash.
