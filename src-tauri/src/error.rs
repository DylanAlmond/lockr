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

    #[error("Sync refused: no User is signed in on this Device")]
    SyncNotLoggedIn,

    #[error("Sync refused: Device {0} could not be reached")]
    SyncPeerUnreachable(String),

    #[error("Sync refused: Device {0} has been revoked")]
    SyncPeerRevoked(String),

    #[error("Sync refused: Device {0} is not paired with this Device")]
    SyncPeerNotPaired(String),

    #[error("Sync refused: a sync is already in progress on this Device")]
    SyncBusy,

    #[error("Sync was cancelled before it completed")]
    SyncCancelled,

    #[error("Sync timed out: {0}")]
    SyncTimeout(String),

    #[error("This Device has been revoked and may no longer sync: {0}")]
    SyncThisDeviceRevoked(String),

    #[error("Sync authentication failed")]
    SyncAuthenticationFailed,

    #[error("Cannot sync with a Device speaking protocol {remote} (this Device speaks {local})")]
    SyncProtocolMismatch { local: u16, remote: u16 },

    #[error("Cannot sync with a Device using vault schema {remote} (this Device uses {local})")]
    SyncSchemaMismatch { local: u32, remote: u32 },

    #[error("Sync discovery failed: {0}")]
    SyncDiscoveryFailed(String),

    #[error("Sync transport is unavailable: {0}")]
    SyncTransportUnavailable(String),

    #[error("Sync was interrupted before it completed: {0}")]
    SyncInterrupted(String),

    #[error("Sync refused: the other Device sent vault {remote} where {local} was expected")]
    SyncVaultIdMismatch { local: String, remote: String },

    #[error("Sync refused: the connection did not come from the expected Device")]
    SyncWrongPeer,

    #[error("Sync refused: {0}")]
    SyncRefused(String),

    #[error("Conflict resolution failed: {0}")]
    SyncConflictResolution(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Validation Error: {0}")]
    Validation(String),
}

impl VaultError {
    /// A stable, machine-readable discriminator for this failure.
    ///
    /// UI code matches on this instead of the human-readable `Display` text,
    /// so rewording a message can never silently collapse two failure states.
    /// These strings are part of the command contract: treat them as API.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "vault.not_found",
            Self::ItemNotFound(_) => "item.not_found",
            Self::UserNotFound(_) => "user.not_found",
            Self::AuthenticationFailed => "auth.wrong_password",
            Self::VaultLocked => "vault.locked",
            Self::Encryption(_) => "crypto.encryption",
            Self::Decryption(_) => "crypto.decryption",
            Self::ExternalChange(_) => "vault.external_change",
            Self::DeviceIdentityDamaged(_) => "device.identity_damaged",
            Self::DeviceRegistry(_) => "device.registry",
            Self::PairingSecretStale => "pairing.secret_stale",
            Self::PairingSchemaMismatch { .. } => "pairing.schema_mismatch",
            Self::PairingRateLimited => "pairing.rate_limited",
            Self::PairingNotStarted => "pairing.not_started",
            Self::Pairing(_) => "pairing.failed",
            Self::SyncNotLoggedIn => "sync.not_logged_in",
            Self::SyncPeerUnreachable(_) => "sync.device_unreachable",
            Self::SyncPeerRevoked(_) => "sync.device_revoked",
            Self::SyncPeerNotPaired(_) => "sync.device_not_paired",
            Self::SyncBusy => "sync.busy",
            Self::SyncCancelled => "sync.cancelled",
            Self::SyncTimeout(_) => "sync.timeout",
            Self::SyncThisDeviceRevoked(_) => "sync.this_device_revoked",
            Self::SyncAuthenticationFailed => "sync.authentication_failed",
            Self::SyncProtocolMismatch { .. } => "sync.protocol_mismatch",
            Self::SyncSchemaMismatch { .. } => "sync.schema_mismatch",
            Self::SyncDiscoveryFailed(_) => "sync.discovery_failed",
            Self::SyncTransportUnavailable(_) => "sync.transport_unavailable",
            Self::SyncInterrupted(_) => "sync.interrupted",
            Self::SyncVaultIdMismatch { .. } => "sync.vault_id_mismatch",
            Self::SyncWrongPeer => "sync.wrong_device",
            Self::SyncRefused(_) => "sync.refused",
            Self::SyncConflictResolution(_) => "sync.conflict_resolution",
            Self::Io(_) => "io",
            Self::Serialization(_) => "serialization",
            Self::Validation(_) => "validation",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::VaultError;

    #[test]
    fn test_sync_failure_codes_are_stable_and_distinct() {
        let cases = [
            (
                VaultError::SyncPeerUnreachable("device".into()),
                "sync.device_unreachable",
            ),
            (
                VaultError::SyncPeerRevoked("device".into()),
                "sync.device_revoked",
            ),
            (VaultError::SyncBusy, "sync.busy"),
            (VaultError::SyncCancelled, "sync.cancelled"),
            (
                VaultError::SyncSchemaMismatch {
                    local: 3,
                    remote: 2,
                },
                "sync.schema_mismatch",
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.code(), expected);
        }

        assert_ne!(
            VaultError::SyncPeerUnreachable("device".into()).code(),
            VaultError::SyncPeerRevoked("device".into()).code(),
            "the UI must be able to tell an unreachable Device from a revoked one"
        );
    }
}
