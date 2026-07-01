use crate::VaultError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use argon2::{Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};

const SALT_LEN: usize = 32;
const NONCE_LEN: usize = 12;

/// Turns a master password into a 256-bit key
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
}
