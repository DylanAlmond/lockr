# Asset tiers and handling requirements

What each tier in the threat model (`threat-model.md` §2) requires of the code.
Decision record for F-23. A new asset must be placed in a tier and inherit the
row's requirements.

Tiers: **T1** compromise means immediate credential loss · **T2** means
targeting or enumeration · **T3** means reconnaissance.

| Requirement | T1 (A1 master password, A2 secret key, A3 secret fields) | T2 (A4 item metadata, A5 user/vault metadata) | T3 (A6 diagnostics) |
|---|---|---|---|
| Encrypted at rest | Yes: vault blob for A3; profile-encrypted secret key for A2 | A4 is in the encrypted vault; A5 profile JSON is plaintext by design | Build artefacts contain no secrets, maps or tokens |
| In safe views | Never; only `get_item_secret` egress | Yes, by design | Not applicable |
| Masked in UI | Yes until explicit reveal; reveal bound 10 s | No (visible by design) | — |
| Clipboard | Only through the core; 30 s compare-and-clear | Normal clipboard | — |
| Logged | Never | Never in `auth.log`; no other log sink | Error strings are generic; no paths/ids |
| Retained on lock/logout | Dropped: vault keys, cached password, secret key; revealed values hidden | Safe views may remain until navigation, but are not secret | Build output only |
| Network | Never | Never | Never |

## Deliberate choices

- **Note bodies and identity PII are T2.** They are visible and copyable by
  design; the product models only field-key secrets as T1. A User who treats a
  note body as secret is outside the model.
- **The card number's last four digits are T2.** The card-number mask shows
  them deliberately (R-S6); the rest of the value is T1.
- **The encrypted secret key is T2 as a blob, T1 as attack material.** It is
  returned to the renderer for the profile list, but it is offline-guessing
  material for the master password; the session and password controls are what
  bound that risk.
- **No T1 value is ever written to a log, an error string, a URL or storage.**

## Adding an asset

1. Assign a tier using the threat model's definitions.
2. Apply the row above; if a requirement cannot be met, record the deviation in
   `findings.md` rather than silently lowering the tier.
3. Add the handling to the public statement in `security-level.md` if it
   changes what a reader should infer.
