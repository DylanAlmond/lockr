# Cryptographic inventory

What Lockr uses, with what parameters, and where. Decision record for F-22.
The correctness of these primitives is not assessed here; the parameters and
uses are.

## 1. Primitives

| Primitive | Parameters | Where | Purpose |
|---|---|---|---|
| Argon2id (v0x13) | m = 64 MiB, t = 3, p = 4, 32-byte output; 32-byte salt | `crypto.rs` `derive_master_key` | Derive a vault master key from `master password ‖ secret key` |
| Argon2id (v0x13) | Same parameters; 32-byte salt | `crypto.rs` `derive_key` | Derive the profile key that wraps the secret key (`(old)` in the comment, but live) |
| AES-256-GCM | 256-bit key, 12-byte random nonce, 128-bit tag | `crypto.rs` `encrypt` / `decrypt` | Encrypt the 32-byte secret key in the user profile |
| AES-256-GCM | 256-bit key, 12-byte random nonce, 128-bit tag | `crypto.rs` `encrypt_with_key` / `decrypt_with_key` | Encrypt the vault blob with the cached master key |
| OS CSPRNG (`OsRng`) | 32 bytes / 12 bytes | `crypto.rs`, `user_manager.rs` | Salt, nonce and secret key generation |
| `zeroize` | — | `MasterKey` (`Zeroizing<[u8; 32]>`), cached password/key, secret key copies | Drop key material on lock/logout; renderer strings cannot be zeroized |

There is no other cryptography: no hashing for integrity outside GCM, no
signatures, no key exchange, no network TLS.

## 2. Key hierarchy

```
master password ─┐
secret key ──────┼─ Argon2id(…, vault salt) ─→ vault master key ─→ AES-256-GCM(vault blob)
                 ┘

master password ─── Argon2id(…, profile salt) ─→ profile key ─→ AES-256-GCM(secret key)
```

- The secret key is random per user and stored only encrypted under the master
  password.
- A vault's salt is random per vault; the profile's salt is random per profile.
- The vault master key is cached in RAM while the vault is unlocked and
  zeroized when it is locked.

## 3. Vault file format

`Base64(salt(32) ‖ nonce(12) ‖ ciphertext)`

The decrypted payload carries a `schema_version` (currently 2). The file has
**no algorithm identifier**: changing the cipher or KDF requires a schema
migration rather than a per-file tag. That is deliberate and recorded here
rather than as a version field (ADR-0001 covers the schema).

## 4. Parameters and rationale

- Argon2id at m = 64 MiB, t = 3, p = 4 is above the OWASP minimum
  (m = 19 MiB, t = 2, p = 1). Each vault unlock pays this once; further saves
  use the cached key.
- AES-256-GCM is an approved AEAD mode. A fresh 12-byte nonce is generated for
  every encryption, including every save.
- `OsRng` is the only randomness source for values that must be unguessable.

## 5. Checks

`static:deps-crypto` asserts that `argon2` and `aes-gcm` resolve in
`src-tauri/Cargo.lock`. The parameter values are asserted by
`rust:crypto-attrs` in `compliance-tests.md` (planned as an integration test).
