use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultError {
    #[error("Vault not found: {0}")]
    NotFound(String),

    #[error("Item not found: {0}")]
    ItemNotFound(String),

    #[error("User not found: {0}")]
    UserNotFound(String),

    #[error("Wrong master password")]
    AuthenticationFailed,

    #[error("Vault is locked")]
    VaultLocked,

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Decryption Error: {0}")]
    Decryption(String),

    #[error("Vault file changed on disk since it was opened: {0}")]
    ExternalChange(String),

    #[error("Device identity is damaged: {0}")]
    DeviceIdentityDamaged(String),

    #[error("Device registry error: {0}")]
    DeviceRegistry(String),

    #[error("Pairing code is stale or has already been used")]
    PairingSecretStale,

    #[error("Pairing refused: incompatible vault schema (local {local}, remote {remote})")]
    PairingSchemaMismatch { local: u32, remote: u32 },

    #[error("Pairing refused: too many attempts, start again")]
    PairingRateLimited,

    #[error("No pairing is in progress")]
    PairingNotStarted,

    #[error("Pairing error: {0}")]
    Pairing(String),

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
