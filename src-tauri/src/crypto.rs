use crate::VaultError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::{Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use zeroize::Zeroize;

const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;

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

/// Derives a 256-bit key for a specific vault using the Master Password + Secret Key
pub fn derive_vault_key(
    master_password: &str,
    secret_key: &[u8],
    salt: &[u8],
) -> Result<[u8; 32], VaultError> {
    let mut combined = Vec::with_capacity(master_password.len() + secret_key.len());
    combined.extend_from_slice(master_password.as_bytes());
    combined.extend_from_slice(secret_key);

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
        .hash_password_into(&combined, salt, &mut key)
        .map_err(|e| VaultError::Encryption(e.to_string()))?;

    // Wipe the combined bytes from memory immediately
    combined.zeroize();

    Ok(key)
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

/// Encrypts data using Master Password + Secret Key (For Vault files)
pub fn encrypt_with_secret(
    master_password: &str,
    secret_key: &[u8],
    plaintext: &[u8],
) -> Result<String, VaultError> {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);

    let key = derive_vault_key(master_password, secret_key, &salt)?;
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

/// Decrypts data using Master Password + Secret Key (For Vault files)
pub fn decrypt_with_secret(
    master_password: &str,
    secret_key: &[u8],
    encrypted_b64: &str,
) -> Result<Vec<u8>, VaultError> {
    let combined = BASE64
        .decode(encrypted_b64)
        .map_err(|e| VaultError::Decryption(e.to_string()))?;

    if combined.len() < SALT_LEN + NONCE_LEN {
        return Err(VaultError::Decryption("Data too short".into()));
    }

    let salt = &combined[..SALT_LEN];
    let nonce = Nonce::try_from(&combined[SALT_LEN..SALT_LEN + NONCE_LEN])
        .map_err(|e| VaultError::Encryption(e.to_string()))?;
    let ciphertext = &combined[SALT_LEN + NONCE_LEN..];

    let key = derive_vault_key(master_password, secret_key, salt)?;
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| VaultError::Decryption(e.to_string()))?;

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

        // Encrypt
        let encrypted = encrypt(password, secret_data).unwrap();

        // It should be base64 (printable ascii, not binary)
        assert!(encrypted.is_ascii());

        // Decrypt
        let decrypted = decrypt(password, &encrypted).unwrap();

        // Should match original
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
    fn test_derive_vault_key_is_deterministic() {
        let mp = "my_master_password";
        let sk = [42u8; 32]; // Mock secret key
        let salt = [99u8; 32]; // Mock salt

        let key1 = derive_vault_key(mp, &sk, &salt).unwrap();
        let key2 = derive_vault_key(mp, &sk, &salt).unwrap();

        assert_eq!(key1.len(), 32, "Key must be exactly 32 bytes");
        assert_eq!(key1, key2, "Same inputs must produce the exact same key");
    }

    #[test]
    fn test_derive_vault_key_changes_with_different_salt() {
        let mp = "my_master_password";
        let sk = [42u8; 32];
        let salt1 = [99u8; 32];
        let mut salt2 = [99u8; 32];
        salt2[0] = 100; // Change one byte in the salt

        let key1 = derive_vault_key(mp, &sk, &salt1).unwrap();
        let key2 = derive_vault_key(mp, &sk, &salt2).unwrap();

        assert_ne!(key1, key2, "Different salts must produce different keys");
    }

    #[test]
    fn test_derive_vault_key_changes_with_different_secret_key() {
        let mp = "my_master_password";
        let sk1 = [42u8; 32];
        let mut sk2 = [42u8; 32];
        sk2[31] = 255; // Change one byte in the secret key
        let salt = [99u8; 32];

        let key1 = derive_vault_key(mp, &sk1, &salt).unwrap();
        let key2 = derive_vault_key(mp, &sk2, &salt).unwrap();

        assert_ne!(
            key1, key2,
            "Different secret keys must produce different keys"
        );
    }

    #[test]
    fn test_vault_key_encrypt_decrypt_round_trip() {
        let mp = "round_trip_password";
        let sk = [15u8; 32];
        let salt = [77u8; 32];
        let secret_data = b"Super secret vault data";

        // Derive the key
        let key = derive_vault_key(mp, &sk, &salt).unwrap();

        // We can't use our normal encrypt() function directly because it generates
        // its own salt. But we CAN manually use the AES-GCM cipher directly to prove
        // the derived key works for standard encryption/decryption.
        use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit, Nonce};

        let cipher = Aes256Gcm::new_from_slice(&key).unwrap();
        let nonce = Nonce::try_from([0u8; 12]).unwrap();

        let ciphertext = cipher.encrypt(&nonce, secret_data.as_ref()).unwrap();
        let plaintext = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();

        assert_eq!(
            secret_data.to_vec(),
            plaintext,
            "Derived key successfully encrypted and decrypted data"
        );
    }
}
