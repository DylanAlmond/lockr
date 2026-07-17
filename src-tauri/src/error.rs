use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultError {
    #[error("Vault not found: {0}")]
    NotFound(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Wrong master password")]
    AuthenticationFailed,

    #[error("Vault is locked")]
    VaultLocked,

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption Error: {0}")]
    Decryption(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Validation Error: {0}")]
    Validation(String),
}

// Tauri requires errors returned from commands to be serializable.
// This converts any VaultError into a simple JSON string.
impl serde::Serialize for VaultError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // We just convert the error to its display string
        serializer.serialize_str(self.to_string().as_ref())
    }
}
