use std::path::Path;

use crate::atomic::write_atomic;
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
            VaultError::Validation(format!(
                "Device id file {} is corrupt; refusing to replace this installation's identity",
                path.display()
            ))
        });
    }

    let device_id = Uuid::new_v4();
    write_atomic(&path, device_id.to_string().as_bytes())?;
    Ok(device_id)
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

        assert!(matches!(result, Err(VaultError::Validation(_))));
        assert_eq!(
            std::fs::read(app_data_dir.join("device_id")).unwrap(),
            b"not-a-uuid",
            "a corrupt identity file must never be silently replaced"
        );

        std::fs::remove_dir_all(app_data_dir).ok();
    }
}
