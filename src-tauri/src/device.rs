use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use zeroize::Zeroize;

use crate::atomic::write_atomic;
use crate::crypto::{decrypt, encrypt};
use crate::{DeviceId, VaultError};

use uuid::Uuid;

/// File name of the persisted installation identity, beside the vaults
/// directory.
const DEVICE_ID_FILE: &str = "device_id";

/// Return this installation's [`DeviceId`], minting and persisting one on
/// first use.
///
/// The id is created as soon as Lockr runs, before any pairing, so that vault
/// content can be stamped with its origin Device. It is written atomically and
/// then reused for the lifetime of the installation's app data directory.
pub fn load_or_create_device_id(app_data_dir: &Path) -> Result<DeviceId, VaultError> {
    std::fs::create_dir_all(app_data_dir)?;

    let path = app_data_dir.join(DEVICE_ID_FILE);

    if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        return Uuid::parse_str(raw.trim()).map_err(|_| {
            VaultError::DeviceIdentityDamaged(format!(
                "{} is not a valid Device id. Lockr will not replace this installation's \
                 identity automatically. Restore the file from a backup, or choose \
                 'start with a new identity' — a one-way action that makes this a new \
                 Device and leaves the old Device's entry revoked.",
                path.display()
            ))
        });
    }

    let device_id = Uuid::new_v4();
    write_atomic(&path, device_id.to_string().as_bytes())?;
    Ok(device_id)
}

/// Explicitly abandon a damaged Device identity and mint a fresh one.
///
/// This is deliberately separate from [`load_or_create_device_id`]: starting
/// again as a new Device is a one-way decision that no honest Device can
/// undo, so it must never happen because a file failed to parse. The damaged
/// file is renamed, not deleted, so the old identity can be inspected or
/// restored by hand.
pub fn reset_device_id(app_data_dir: &Path) -> Result<DeviceId, VaultError> {
    std::fs::create_dir_all(app_data_dir)?;

    let path = app_data_dir.join(DEVICE_ID_FILE);
    if path.exists() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let backup = app_data_dir.join(format!("device_id.damaged-{nanos}"));
        std::fs::rename(&path, backup)?;
    }

    load_or_create_device_id(app_data_dir)
}

/// This Device's per-installation Ed25519 keypair. The private half is only
/// ever held wrapped under the User's master password (see [`wrap`]); the
/// public half is what the Device Registry publishes.
///
/// [`wrap`]: DeviceKeypair::wrap
pub struct DeviceKeypair {
    signing_key: SigningKey,
}

impl DeviceKeypair {
    /// Mint a new Device keypair from the operating system's randomness.
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let signing_key = SigningKey::from_bytes(&bytes);
        bytes.zeroize();
        Self { signing_key }
    }

    /// Recover the keypair from its wrapped form, as stored in the
    /// device-local profile.
    pub fn from_wrapped(master_password: &str, wrapped: &str) -> Result<Self, VaultError> {
        let mut bytes = decrypt(master_password, wrapped)?;
        let key_bytes: [u8; 32] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| VaultError::Decryption("Invalid device signing key length".into()))?;
        bytes.zeroize();
        Ok(Self {
            signing_key: SigningKey::from_bytes(&key_bytes),
        })
    }

    /// Wrap the private half the same way the User's secret key is wrapped:
    /// Argon2id over the master password, then AES-256-GCM.
    pub fn wrap(&self, master_password: &str) -> Result<String, VaultError> {
        encrypt(master_password, self.signing_key.as_bytes())
    }

    /// The Base64 public key published in the Device Registry.
    pub fn public_key(&self) -> String {
        BASE64.encode(self.signing_key.verifying_key().to_bytes())
    }

    /// Sign a canonical payload, returning a Base64 signature.
    pub fn sign(&self, payload: &[u8]) -> String {
        BASE64.encode(self.signing_key.sign(payload).to_bytes())
    }

    #[cfg(test)]
    fn private_bytes(&self) -> &[u8; 32] {
        self.signing_key.as_bytes()
    }
}

/// Verify a Base64 Ed25519 signature against a Base64 public key.
///
/// A malformed key or signature is a verification failure, never a panic:
/// registry entries arrive from peers and are untrusted input.
pub fn verify_signature(
    public_key: &str,
    payload: &[u8],
    signature: &str,
) -> Result<(), VaultError> {
    let key_bytes = BASE64
        .decode(public_key)
        .map_err(|_| VaultError::DeviceRegistry("public key is not valid Base64".into()))?;
    let key_bytes: [u8; 32] = key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| VaultError::DeviceRegistry("public key has the wrong length".into()))?;
    let verifying_key = VerifyingKey::from_bytes(&key_bytes)
        .map_err(|_| VaultError::DeviceRegistry("public key is not a valid Ed25519 key".into()))?;

    let signature_bytes = BASE64
        .decode(signature)
        .map_err(|_| VaultError::DeviceRegistry("signature is not valid Base64".into()))?;
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| VaultError::DeviceRegistry("signature has the wrong length".into()))?;

    verifying_key
        .verify_strict(payload, &signature)
        .map_err(|_| VaultError::DeviceRegistry("signature does not match the entry".into()))
}

/// The name this Device announces when a human approves its pairing.
///
/// It is a best-effort machine name, never a source of identity: admission is
/// by Device id and public key, and the name is display data.
pub fn device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "Lockr Device".to_string())
}

/// The platform this Device announces, for the approval prompt.
pub fn device_platform() -> String {
    std::env::consts::OS.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> std::path::PathBuf {
        let unique_id = Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("device_id_test_{}", unique_id));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_device_id_is_minted_once_and_stable() {
        let app_data_dir = setup();

        let first = load_or_create_device_id(&app_data_dir).unwrap();
        let second = load_or_create_device_id(&app_data_dir).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            std::fs::read_to_string(app_data_dir.join("device_id"))
                .unwrap()
                .trim(),
            first.to_string()
        );

        std::fs::remove_dir_all(app_data_dir).ok();
    }

    #[test]
    fn test_device_ids_are_isolated_per_app_data_dir() {
        let first_dir = setup();
        let second_dir = setup();

        let first = load_or_create_device_id(&first_dir).unwrap();
        let second = load_or_create_device_id(&second_dir).unwrap();

        assert_ne!(first, second);

        std::fs::remove_dir_all(first_dir).ok();
        std::fs::remove_dir_all(second_dir).ok();
    }

    #[test]
    fn test_corrupt_device_id_is_reported_not_replaced() {
        let app_data_dir = setup();
        std::fs::write(app_data_dir.join("device_id"), b"not-a-uuid").unwrap();

        let result = load_or_create_device_id(&app_data_dir);

        assert!(matches!(result, Err(VaultError::DeviceIdentityDamaged(_))));
        assert_eq!(
            std::fs::read(app_data_dir.join("device_id")).unwrap(),
            b"not-a-uuid",
            "a corrupt identity file must never be silently replaced"
        );

        std::fs::remove_dir_all(app_data_dir).ok();
    }

    #[test]
    fn test_device_identity_can_be_reset_explicitly_preserving_the_damaged_file() {
        let app_data_dir = setup();
        std::fs::write(app_data_dir.join("device_id"), b"not-a-uuid").unwrap();

        let new_id = reset_device_id(&app_data_dir).unwrap();

        assert_eq!(
            std::fs::read_to_string(app_data_dir.join("device_id"))
                .unwrap()
                .trim(),
            new_id.to_string()
        );

        let damaged: Vec<_> = std::fs::read_dir(&app_data_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("device_id.damaged-")
            })
            .collect();
        assert_eq!(damaged.len(), 1, "the damaged file must be kept aside");
        assert_eq!(
            std::fs::read(damaged[0].path()).unwrap(),
            b"not-a-uuid",
            "reset must not destroy evidence of the damaged identity"
        );

        std::fs::remove_dir_all(app_data_dir).ok();
    }

    #[test]
    fn test_device_keypair_roundtrips_through_its_wrapped_private_half() {
        let keypair = DeviceKeypair::generate();
        let wrapped = keypair.wrap("MasterPass123!").unwrap();

        let restored = DeviceKeypair::from_wrapped("MasterPass123!", &wrapped).unwrap();

        assert_eq!(restored.public_key(), keypair.public_key());
        assert_eq!(
            restored.sign(b"registry entry"),
            keypair.sign(b"registry entry")
        );
        assert!(!wrapped.contains(&BASE64.encode(keypair.private_bytes())));
    }

    #[test]
    fn test_wrapped_device_keypair_rejects_a_wrong_password() {
        let keypair = DeviceKeypair::generate();
        let wrapped = keypair.wrap("RightPassword").unwrap();

        let result = DeviceKeypair::from_wrapped("WrongPassword", &wrapped);

        assert!(matches!(result, Err(VaultError::AuthenticationFailed)));
    }

    #[test]
    fn test_signature_verification_rejects_a_tampered_payload() {
        let keypair = DeviceKeypair::generate();
        let signature = keypair.sign(b"original");

        assert!(verify_signature(&keypair.public_key(), b"original", &signature).is_ok());
        assert!(verify_signature(&keypair.public_key(), b"tampered", &signature).is_err());
        assert!(verify_signature("not-a-key", b"original", &signature).is_err());
    }
}
