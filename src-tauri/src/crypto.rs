use crate::VaultError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::{Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use zeroize::{Zeroize, Zeroizing};

pub const SALT_LEN: usize = 32;
pub const NONCE_LEN: usize = 12;

/// A derived 256-bit master key. Held in memory only while the vault is
/// unlocked. Automatically zeroized when dropped.
pub struct MasterKey(Zeroizing<[u8; 32]>);

impl MasterKey {
    #[inline]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Generates a cryptographically random salt for a new vault.
pub fn generate_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Derives a MasterKey from master password + secret key + salt.
/// This is an expensive Argon2id call — should be called once per vault
pub fn derive_master_key(
    master_password: &str,
    secret_key: &[u8],
    salt: &[u8],
) -> Result<MasterKey, VaultError> {
    let mut combined = Vec::with_capacity(master_password.len() + secret_key.len());
    combined.extend_from_slice(master_password.as_bytes());
    combined.extend_from_slice(secret_key);

    let params = Params::new(
        64 * 1024, // 64 MB memory
        3,         // 3 iterations
        4,         // 4 threads
        Some(32),  // 32-byte output
    )
    .map_err(|e| VaultError::Encryption(e.to_string()))?;

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);

    let mut key = Zeroizing::new([0u8; 32]);
    argon2
        .hash_password_into(&combined, salt, key.as_mut_slice())
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    // Wipe the combined bytes from memory immediately
    combined.zeroize();

    Ok(MasterKey(key))
}

/// Turns a master password into a 256-bit key (old)
pub fn derive_key(master_password: &str, salt: &[u8]) -> Result<[u8; 32], VaultError> {
    let params = Params::new(
        64 * 1024, // Memory: 64 MB
        3,         // Iterations: 3
        4,         // Parallelism: 4 threads
        Some(32),  // Output key length: 32 bytes (256 bits)
    )
    .map_err(|e| VaultError::Encryption(e.to_string()))?;

    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; 32];
    argon2
        .hash_password_into(master_password.as_bytes(), salt, &mut key)
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    Ok(key)
}

/// Encrypts plaintext using an already-derived MasterKey.
/// Format: `salt(32) + nonce(12) + ciphertext` → Base64
pub fn encrypt_with_key(
    key: &MasterKey,
    salt: &[u8; SALT_LEN],
    plaintext: &[u8],
) -> Result<String, VaultError> {
    let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::try_from(nonce_bytes.as_slice())
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    let mut combined = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(salt);
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypts a Base64 payload using an already-derived MasterKey.
/// Returns `(plaintext, salt)` so the caller can cache the salt for
/// future `encrypt_with_key` calls.
pub fn decrypt_with_key(
    key: &MasterKey,
    encrypted_b64: &str,
) -> Result<(Vec<u8>, [u8; SALT_LEN]), VaultError> {
    let combined = BASE64
        .decode(encrypted_b64)
        .map_err(|e| VaultError::Decryption(e.to_string()))?;

    if combined.len() < SALT_LEN + NONCE_LEN {
        return Err(VaultError::Decryption("Data too short".into()));
    }

    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&combined[..SALT_LEN]);

    let nonce = Nonce::try_from(&combined[SALT_LEN..SALT_LEN + NONCE_LEN])
        .map_err(|e| VaultError::Encryption(e.to_string()))?;
    let ciphertext = &combined[SALT_LEN + NONCE_LEN..];

    let cipher = Aes256Gcm::new_from_slice(key.as_bytes())
        .map_err(|e| VaultError::Decryption(e.to_string()))?;

    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| VaultError::AuthenticationFailed)?;

    Ok((plaintext, salt))
}

/// Extracts just the salt from an encrypted payload without decrypting.
/// Used at unlock time to derive the MasterKey before decryption.
pub fn extract_salt(encrypted_b64: &str) -> Result<[u8; SALT_LEN], VaultError> {
    let combined = BASE64
        .decode(encrypted_b64)
        .map_err(|e| VaultError::Decryption(e.to_string()))?;

    if combined.len() < SALT_LEN {
        return Err(VaultError::Decryption("Data too short".into()));
    }

    let mut salt = [0u8; SALT_LEN];
    salt.copy_from_slice(&combined[..SALT_LEN]);
    Ok(salt)
}

/// Encrypts plaintext bytes, returns a Base64 string
pub fn encrypt(master_password: &str, plaintext: &[u8]) -> Result<String, VaultError> {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);

    let key = derive_key(master_password, &salt)?;

    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| VaultError::Encryption(e.to_string()))?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::try_from(nonce_bytes.as_slice())
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    let mut combined = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(&salt);
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(BASE64.encode(&combined))
}

/// Decrypts a Base64 string back to plaintext bytes
pub fn decrypt(master_password: &str, encrypted_b64: &str) -> Result<Vec<u8>, VaultError> {
    let combined = BASE64
        .decode(encrypted_b64)
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    if combined.len() < SALT_LEN + NONCE_LEN {
        return Err(VaultError::Decryption("Data too short".into()));
    }

    let salt = &combined[..SALT_LEN];
    let nonce = Nonce::try_from(&combined[SALT_LEN..SALT_LEN + NONCE_LEN])
        .map_err(|e| VaultError::Encryption(e.to_string()))?;
    let ciphertext = &combined[SALT_LEN + NONCE_LEN..];

    let key = derive_key(master_password, salt)?;

    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| VaultError::Encryption(e.to_string()))?;

    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| VaultError::AuthenticationFailed)?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let password = "my-super-secret-master-password";
        let secret_data = b"{\"username\": \"admin\", \"password\": \"hunter2\"}";

        let encrypted = encrypt(password, secret_data).unwrap();
        assert!(encrypted.is_ascii());

        let decrypted = decrypt(password, &encrypted).unwrap();
        assert_eq!(secret_data.to_vec(), decrypted);
    }

    #[test]
    fn test_wrong_password_fails() {
        let password = "correct-password";
        let wrong_password = "wrong-password";
        let secret_data = b"secret stuff";

        let encrypted = encrypt(password, secret_data).unwrap();
        let result = decrypt(wrong_password, &encrypted);

        assert!(result.is_err());
        assert!(matches!(result, Err(VaultError::AuthenticationFailed)));
    }

    #[test]
    fn test_master_key_roundtrip() {
        let mp = "my_master_password";
        let sk = [42u8; 32];
        let salt = generate_salt();
        let secret_data = b"Super secret vault data";

        // Derive key once (expensive)
        let key = derive_master_key(mp, &sk, &salt).unwrap();

        // Encrypt with cached key (fast)
        let encrypted = encrypt_with_key(&key, &salt, secret_data).unwrap();

        // Decrypt with cached key (fast)
        let (decrypted, extracted_salt) = decrypt_with_key(&key, &encrypted).unwrap();

        assert_eq!(secret_data.to_vec(), decrypted);
        assert_eq!(salt, extracted_salt);
    }

    #[test]
    fn test_extract_salt_matches() {
        let mp = "test_password";
        let sk = [99u8; 32];
        let salt = generate_salt();
        let key = derive_master_key(mp, &sk, &salt).unwrap();

        let encrypted = encrypt_with_key(&key, &salt, b"data").unwrap();

        let extracted = extract_salt(&encrypted).unwrap();
        assert_eq!(salt, extracted);
    }

    #[test]
    fn test_different_salts_produce_different_keys() {
        let mp = "my_master_password";
        let sk = [42u8; 32];
        let salt1 = generate_salt();
        let salt2 = generate_salt();

        let key1 = derive_master_key(mp, &sk, &salt1).unwrap();
        let key2 = derive_master_key(mp, &sk, &salt2).unwrap();

        assert_ne!(
            key1.as_bytes(),
            key2.as_bytes(),
            "Different salts must produce different keys"
        );
    }

    #[test]
    fn test_fresh_nonce_each_encryption() {
        let mp = "test";
        let sk = [0u8; 32];
        let salt = generate_salt();
        let key = derive_master_key(mp, &sk, &salt).unwrap();

        let enc1 = encrypt_with_key(&key, &salt, b"same data").unwrap();
        let enc2 = encrypt_with_key(&key, &salt, b"same data").unwrap();

        // Same plaintext + same key, but different nonce → different ciphertext
        assert_ne!(enc1, enc2, "Each encryption must use a fresh nonce");

        // Both should decrypt back to the same plaintext
        let (dec1, _) = decrypt_with_key(&key, &enc1).unwrap();
        let (dec2, _) = decrypt_with_key(&key, &enc2).unwrap();
        assert_eq!(dec1, dec2);
    }
}
