use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
};

use uuid::Uuid;

use crate::VaultError;

/// Write `contents` to `target` atomically.
///
/// The bytes are written to a temporary file beside the target and fsynced,
/// then renamed over the target. A reader therefore sees either the previous
/// file or the complete new one, never a partial write.
pub fn write_atomic(target: &Path, contents: &[u8]) -> Result<(), VaultError> {
    write_atomic_before_rename(target, contents, |_, _| Ok(()))
}

/// Implementation of [`write_atomic`] with a seam for the interval between
/// writing the temporary file and renaming it over the target.
///
/// `before_rename` runs after the temporary file has been fsynced. Tests use
/// it to simulate the process dying mid-save.
fn write_atomic_before_rename<F>(
    target: &Path,
    contents: &[u8],
    before_rename: F,
) -> Result<(), VaultError>
where
    F: FnOnce(&Path, &Path) -> Result<(), VaultError>,
{
    let temp = temp_path(target);

    // Write the complete new contents to a sibling file and force them to
    // disk before anything touches the target.
    let write_result = (|| -> Result<(), VaultError> {
        let mut file = File::create(&temp)?;
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(())
    })();

    if let Err(err) = write_result {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }

    if let Err(err) = before_rename(&temp, target) {
        let _ = fs::remove_file(&temp);
        return Err(err);
    }

    // Renaming replaces the target in one step, so a reader sees either the
    // old file or the complete new one.
    if let Err(err) = fs::rename(&temp, target) {
        let _ = fs::remove_file(&temp);
        return Err(err.into());
    }

    Ok(())
}

/// A unique temporary path in the same directory as `target`, so the final
/// rename never crosses a filesystem boundary.
fn temp_path(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("write");

    target.with_file_name(format!(".{}.tmp-{}", file_name, Uuid::new_v4()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn setup() -> PathBuf {
        let unique_id = Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("atomic_write_test_{}", unique_id));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    #[test]
    fn test_write_atomic_creates_new_file() {
        let temp_dir = setup();
        let target = temp_dir.join("vault.vault");

        write_atomic(&target, b"first contents").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"first contents");

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_write_atomic_replaces_existing_file() {
        let temp_dir = setup();
        let target = temp_dir.join("vault.vault");
        std::fs::write(&target, b"previous contents").unwrap();

        write_atomic(&target, b"replacement contents").unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"replacement contents");

        // No temporary files are left beside the target
        let entries: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(entries, vec![target]);

        std::fs::remove_dir_all(temp_dir).ok();
    }

    #[test]
    fn test_interrupted_write_leaves_previous_file_intact() {
        let temp_dir = setup();
        let target = temp_dir.join("vault.vault");
        std::fs::write(&target, b"previous contents").unwrap();

        // Simulate the process dying after the temp file was written and
        // fsynced but before the rename replaced the target.
        let result = write_atomic_before_rename(&target, b"half a save", |_, _| {
            Err(VaultError::Io(std::io::Error::new(
                std::io::ErrorKind::Other,
                "simulated crash",
            )))
        });

        assert!(result.is_err(), "the interrupted save must report failure");
        assert_eq!(
            std::fs::read(&target).unwrap(),
            b"previous contents",
            "the previous file must still be readable"
        );

        // The partial temp file must not be left behind
        let entries: Vec<_> = std::fs::read_dir(&temp_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(entries, vec![target]);

        std::fs::remove_dir_all(temp_dir).ok();
    }
}
