# ADR-0003: Sync follow-ups from the implementation review

- **Status:** Accepted
- **Date:** 2026-11-13
- **Deciders:** DylanAlmond

## Context

Device sync is implemented on `feat/device-sync` (ADR-0002). A two-axis review of
that branch — standards and spec — plus the per-ticket merge audits surfaced
limitations that ADR-0002 either accepted provisionally or did not address. This
ADR records what happens to each of them, so they stop being footnotes on closed
tickets.

## Decision

### 1. Revocation ordering: accepted for now, rotation is the intended fix

A revocation carries no causal context, so verification cannot tell one signed
while its signer was admitted from one forged afterwards by a revoked Device.
Rejecting records by *arrival* order was considered and rejected: arrival order
differs per Device, so honest Devices would permanently disagree about whether a
Device is revoked, breaking the determinism merge depends on.

The intended fix is **key rotation on revocation**: revoking rotates the account's
governance authority, recorded in the registry, so a revoked Device's old key
cannot authorise anything accepted as new. It is *staged* rather than built now
because its failure modes are expensive: it needs an admitted, unlocked Device to
perform, it can split the account if two Devices rotate concurrently, and its only
escape hatch is physical re-pairing. That is a bad set of properties to add to the
one action a user reaches for when something has already gone wrong.

When it is built, the contract is fixed:

- No dialogs. Revoking stays one action.
- The copy says revocation is confirmed by other Devices on their next sync.
- Concurrent rotation resolves deterministically: highest epoch wins; equal epochs
  are broken by the lower hash of the new public key.
- Physical re-pairing is the documented escape hatch from a split account.

Until then, ADR-0002 §8's limitation stands and is a **design decision**, not a
pending defect. The guard exists at issuance; a modified client can bypass it.

### 2. Tombstone compaction is acknowledgement-based

A Tombstone may be forgotten once every currently-admitted Device has
acknowledged a clock covering it. Each Device publishes its applied clock in the
shared registry; revoked Devices are excluded from the acknowledgement set, which
is safe because they can never sync again.

A Device that never returns therefore holds up compaction indefinitely. That is
accepted: tombstones are tiny, and the alternative deletes a deletion a live
Device still needs.

### 3. Cross-vault moves reconcile after the per-Vault merge

A move leaves a Move Record in the source Vault naming its destination. Merging
runs per Vault, so it cannot see that destination. After the per-Vault merge of a
sync session, the engine reconciles Move Records against destinations present in
the same session. No protocol change and no new state: both sides perform the same
walk, so they converge.

### 4. Transport hardening

- **Cancellation is observed in slices.** Reads use a short deadline and re-check
  the cancel flag between slices, so cancelling costs a fraction of a second
  instead of up to the full frame timeout.
- **The pairing attempt budget is per source address**, and a failed attempt never
  invalidates the open offer. The 40-bit typed fallback is retained: with a
  per-source budget it is adequate, and lengthening it buys little.
- **The Device Directory snapshot is refreshed per connection**, not once per
  session, so a long-lived session cannot authenticate against a stale view.

### 5. Conflicts say which side is local

Each conflict side carries whether it was written by this Device. A conflict with
no local side is surfaced as "this conflict arrived from other Devices; sync again
to refresh" with a **manual** re-sync action. Sync is user-initiated everywhere
else in this app, and an automatic retry against a peer that may be offline turns
one clear error into a spinner followed by a different error.

### 6. Dependencies and licensing

`ed25519-dalek` is retained for registry signing; `ring` was considered as a
replacement to remove the BSD-3 obligation, but the working signing layer is not
worth rewriting for that alone. A `THIRD-PARTY-NOTICES` file is generated from the
actual lockfile, covering direct and transitive licences.

### 7. Verification

The manual two-machine script moves into the repository rather than living only in
a closed issue. A minimal frontend harness is added for exactly one invariant —
secret values never reach rendered or reactive state outside an explicit reveal.
Everything else in the UI remains human-verified.

The harness tests **pure logic only**: redaction decisions, error classification,
and reveal/hide transitions, extracted so they take plain data and return plain
data. It does not mock Tauri's `invoke` and does not mount components. Mocking the
IPC boundary would test the mock more than the code, and component tests would
drift from the one invariant worth guarding.

### 8. The deferred refactors are scheduled, not forgotten

`DevicesView.vue` is split; the four `ItemKind × field-key` cascades collapse into
one table with read/write accessors; `InMemory*` moves behind `#[cfg(test)]` so it
stops shipping; `parts_mut()`'s four-tuple becomes a `SessionCredentials` type.

### 9. Sequence

Behaviour first, refactors after. The four refactors touch `DevicesView.vue`,
`models.rs`/`merge.rs` and `commands.rs` — the same files compaction, move
reconciliation and transport hardening touch. Landing both at once would produce
wide mechanical conflicts that hide semantic ones.

Order: tombstone compaction and acknowledgements; cross-vault move reconciliation;
transport hardening and conflict side identity; then the refactors and the
frontend harness; then the documentation. Key rotation is tracked separately and
not scheduled.

## Consequences

- Revocation has a known, documented window until rotation lands. That window must
  be stated in user-facing copy, not only in this file.
- Compaction depends on an acknowledgement that does not exist yet; until it does,
  tombstones accumulate and nothing is lost.
- The reconciliation pass gives sync a whole-session step it did not have before.
  It must not become a place where cross-vault invariants are quietly enforced.
- Adding a frontend harness creates an expectation of frontend tests. It is scoped
  to the redaction invariant deliberately; other UI behaviour stays manual.
- A `THIRD-PARTY-NOTICES` file must be regenerated whenever dependencies change,
  or it becomes a lie.
