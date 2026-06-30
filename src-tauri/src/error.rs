use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultError {
    #[error("Vault not found: {0}")]
    NotFound(String),

    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("Wrong master password")]
    AuthenticationFailed,

    #[error("Vault is locked")]
    VaultLocked,

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
