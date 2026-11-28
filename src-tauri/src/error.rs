use thiserror::Error;

#[derive(Error, Debug)]
pub enum VaultError {
    #[error("Vault not found")]
    NotFound(String),

    #[error("Item not found")]
    ItemNotFound(String),

    #[error("User not found")]
    UserNotFound(String),

    #[error("Wrong master password")]
    AuthenticationFailed,

    /// Blocked; the payload is a human-readable retry delay.
    #[error("Too many failed login attempts; try again in {0}")]
    TooManyAttempts(String),

    #[error("Vault is locked")]
    VaultLocked,

    #[error("Not authorised")]
    Unauthorised,

    #[error("Encryption failed")]
    Encryption(String),

    #[error("Decryption failed")]
    Decryption(String),

    /// The path and OS message stay in `Debug`/`source`; Display is what
    /// crosses IPC and must not leak the filesystem layout.
    #[error("A filesystem operation failed")]
    Io(#[from] std::io::Error),

    #[error("The data format is invalid")]
    Serialization(#[from] serde_json::Error),

    #[error("Validation Error: {0}")]
    Validation(String),
}

// Tauri requires errors returned from commands to be serializable.
// This converts any VaultError into a generic, non-leaking string.
impl serde::Serialize for VaultError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_display_does_not_leak_the_path() {
        let error = VaultError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "C:\\Users\\alice\\lockr\\vaults\\secret.vault",
        ));

        let message = error.to_string();
        assert_eq!(message, "A filesystem operation failed");
        assert!(!message.contains("secret.vault"));
    }

    #[test]
    fn test_not_found_display_hides_the_identifier() {
        assert_eq!(
            VaultError::NotFound("0b4f4c2e-0000-0000-0000-000000000000".into()).to_string(),
            "Vault not found"
        );
        assert_eq!(
            VaultError::ItemNotFound("0b4f4c2e-0000-0000-0000-000000000000".into()).to_string(),
            "Item not found"
        );
        assert_eq!(
            VaultError::UserNotFound("0b4f4c2e-0000-0000-0000-000000000000".into()).to_string(),
            "User not found"
        );
    }
}
