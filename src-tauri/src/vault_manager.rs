use std::{
    collections::HashMap,
    fs::read_dir,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use chrono::Utc;
use uuid::Uuid;
use zxcvbn::{zxcvbn, Entropy};

use crate::atomic::write_atomic;
use crate::crypto::{
    decrypt_with_key, derive_master_key, encrypt_with_key, generate_salt, MasterKey, SALT_LEN,
};
use crate::device::load_or_create_device_id;
use crate::{
    allowed_secret_keys, item_to_safe, CURRENT_SCHEMA_VERSION, DeviceId,
    FieldVersion, IntoSafe, ItemFilter, ItemId, ItemKind, ItemSecrets, LegacyVault, NewItemData,
    SafeItem, SafeVault, SecretInput, Vault, VaultError, VaultId, VaultItem, VersionVector,
    SECRET_FIELD_PASSWORD,
};

/// Normalizes an optional text field: empty strings mean "not set".
fn normalize(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
}

/// Rejects bad kind data before an update mutates anything. In particular,
/// the lifecycle clock must not advance on a write that is about to fail.
fn validate_kind_data(current: &ItemKind, data: &NewItemData) -> Result<(), VaultError> {
    match (current, data) {
        (ItemKind::Account(_), NewItemData::Account(_))
        | (ItemKind::Note(_), NewItemData::Note(_))
        | (ItemKind::Identity(_), NewItemData::Identity(_)) => Ok(()),
        (ItemKind::Card(_), NewItemData::Card(card)) => {
            if !(1..=12).contains(&card.expiry_month) {
                return Err(VaultError::Validation(format!(
                    "Card expiry month must be between 1 and 12, got {}",
                    card.expiry_month
                )));
            }
            Ok(())
        }
        _ => Err(VaultError::Validation(
            "Changing an item's kind is not supported".to_string(),
        )),
    }
}

/// The kind-specific wire fields that actually differ between the current
/// data and the proposed replacement. Stamping only these keeps per-field
/// clocks honest: changing an account's username must not restamp its email.
fn changed_kind_fields(current: &ItemKind, data: &NewItemData) -> Vec<&'static str> {
    let mut changed = Vec::new();

    match (current, data) {
        (ItemKind::Account(current), NewItemData::Account(new)) => {
            if current.username != new.username {
                changed.push("username");
            }
            if current.email != new.email {
                changed.push("email");
            }
        }
        (ItemKind::Note(current), NewItemData::Note(new)) => {
            if current.title != new.title {
                changed.push("title");
            }
            if current.body != new.body {
                changed.push("body");
            }
        }
        (ItemKind::Card(current), NewItemData::Card(new)) => {
            if current.cardholder_name != new.cardholder_name {
                changed.push("cardholder_name");
            }
            if current.expiry_month != new.expiry_month {
                changed.push("expiry_month");
            }
            if current.expiry_year != new.expiry_year {
                changed.push("expiry_year");
            }
            if current.brand != new.brand {
                changed.push("brand");
            }
        }
        (ItemKind::Identity(current), NewItemData::Identity(new)) => {
            if current.full_name != new.full_name {
                changed.push("full_name");
            }
            if current.email != new.email {
                changed.push("email");
            }
            if current.phone != new.phone {
                changed.push("phone");
            }
            if current.address_line1 != new.address_line1 {
                changed.push("address_line1");
            }
            if current.address_line2 != new.address_line2 {
                changed.push("address_line2");
            }
            if current.city != new.city {
                changed.push("city");
            }
            if current.postal_code != new.postal_code {
                changed.push("postal_code");
            }
            if current.country != new.country {
                changed.push("country");
            }
        }
        // Kind mismatches are rejected by `validate_kind_data` first.
        _ => {}
    }

    changed
}

/// Refuse to save over a file whose contents no longer match what the
/// manager last read or wrote. `None` means the file must not exist yet.
fn ensure_file_unchanged(
    path: &Path,
    expected: Option<u64>,
    vault_id: VaultId,
) -> Result<(), VaultError> {
    match std::fs::read(path) {
        Ok(bytes) if expected == Some(fingerprint(&bytes)) => Ok(()),
        Ok(_) => Err(VaultError::ExternalChange(vault_id.to_string())),
        Err(err) if err.kind() == ErrorKind::NotFound => match expected {
            None => Ok(()),
            Some(_) => Err(VaultError::ExternalChange(vault_id.to_string())),
        },
        Err(err) => Err(err.into()),
    }
}

/// A non-cryptographic content fingerprint used to notice that a vault file
/// was replaced since it was last read or written. It guards against a second
/// writer or an accidental hand edit; the file is encrypted and authenticated
/// against deliberate tampering already.
fn fingerprint(bytes: &[u8]) -> u64 {
    // FNV-1a, 64-bit
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The last writer of one field, as this Device currently has it. Used to
/// refuse a stale Conflict choice: the value the User was shown is only valid
/// while this version is unchanged.
fn current_field_version(
    vault: &Vault,
    scope: &crate::ConflictScope,
    field_key: &str,
) -> Option<FieldVersion> {
    match scope {
        crate::ConflictScope::Item { item_id, .. } => vault
            .items
            .iter()
            .find(|item| item.id == *item_id && !item.is_content_free())
            .and_then(|item| item.field_clocks.get(field_key).copied()),
        crate::ConflictScope::Vault => match field_key {
            "name" => Some(vault.name_clock),
            "color" => Some(vault.color_clock),
            _ => None,
        },
    }
}

/// An unlocked vault held in RAM, along with its cached encryption key,
/// salt, and dirty flag.
///
/// When this struct is dropped, `MasterKey` is automatically zeroized.
struct UnlockedVault {
    vault: Vault,
    master_key: MasterKey,
    salt: [u8; SALT_LEN],
    dirty: bool,
    /// Fingerprint of the encrypted file as last read from or written to
    /// disk. `None` means the file is not expected to exist yet.
    fingerprint: Option<u64>,
}

pub struct VaultManager {
    /// Where to store vault files
    vaults_dir: PathBuf,

    /// This installation's identity, minted on first use and persisted
    /// beside the vaults directory.
    device_id: DeviceId,

    /// A map of decrypted unlocked vaults (with cached keys)
    unlocked_vaults: HashMap<VaultId, UnlockedVault>,

    /// When true (default), mutations save immediately.
    /// When false, mutations only mark dirty — caller must flush.
    autosave: bool,
}

impl VaultManager {
    /// Create new vault dir
    pub fn new(app_data_dir: PathBuf) -> Result<Self, VaultError> {
        let device_id = load_or_create_device_id(&app_data_dir)?;
        let vaults_dir = app_data_dir.join("vaults");

        if !vaults_dir.exists() {
            std::fs::create_dir_all(&vaults_dir)?;
        }

        Ok(Self {
            vaults_dir,
            device_id,
            unlocked_vaults: HashMap::new(),
            autosave: true,
        })
    }

    /// This installation's Device identity.
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Toggle autosave. When disabled, mutations only mark vaults dirty.
    /// Call `flush_all()` (or `flush_vault`) to persist changes.
    pub fn set_autosave(&mut self, enabled: bool) {
        self.autosave = enabled;
    }

    /// Get path to a vault file
    fn vault_path(&self, vault_id: VaultId) -> PathBuf {
        self.vaults_dir.join(format!("{}.vault", vault_id))
    }

    /// Get path to the backup a vault migration writes. Backups are only
    /// ever written at migration time, never on plain unlocks.
    fn vault_backup_path(&self, vault_id: VaultId) -> PathBuf {
        self.vaults_dir.join(format!("{}.vault.OLD", vault_id))
    }

    /// Where a Vault deleted on another Device becomes local trash. The purge
    /// is left to the User; a remote trigger never hard-deletes.
    pub(crate) fn vault_trash_path(&self, vault_id: VaultId) -> PathBuf {
        self.vaults_dir.join(format!("{}.vault.deleted", vault_id))
    }

    /// Check if a SPECIFIC vault is unlocked
    fn is_vault_unlocked(&self, vault_id: VaultId) -> bool {
        self.unlocked_vaults.contains_key(&vault_id)
    }

    /// Get an immutable reference to a specific unlocked vault's data
    pub fn get_vault(&self, vault_id: VaultId) -> Result<&Vault, VaultError> {
        self.unlocked_vaults
            .get(&vault_id)
            .map(|uv| &uv.vault)
            .ok_or(VaultError::VaultLocked)
    }

    /// Get a mutable reference to a specific unlocked vault's data.
    /// Also marks the vault as dirty.
    fn get_vault_mut(&mut self, vault_id: VaultId) -> Result<&mut Vault, VaultError> {
        let uv = self
            .unlocked_vaults
            .get_mut(&vault_id)
            .ok_or(VaultError::VaultLocked)?;
        uv.dirty = true;
        Ok(&mut uv.vault)
    }

    /// Returns a list of existing vault IDs (filenames) without unlocking them
    pub fn list_vault_ids(&self) -> Result<Vec<String>, VaultError> {
        let mut ids = Vec::new();

        if !self.vaults_dir.exists() {
            return Ok(ids);
        }

        for entry in read_dir(&self.vaults_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(extension) = path.extension() {
                if extension == "vault" {
                    if let Some(stem) = path.file_stem() {
                        if let Some(id_str) = stem.to_str() {
                            ids.push(id_str.to_string());
                        }
                    }
                }
            }
        }

        Ok(ids)
    }

    /// Returns a list of all currently unlocked vaults
    pub fn get_unlocked_vaults(&self) -> Vec<SafeVault> {
        self.unlocked_vaults
            .values()
            .map(|uv| uv.vault.into_safe())
            .collect()
    }

    /// Create a new vault, derive master key (Argon2id — expensive),
    /// cache it, and save to disk.
    pub fn create_vault(
        &mut self,
        name: String,
        color: String,
        master_password: &str,
        secret_key: &[u8],
    ) -> Result<Vault, VaultError> {
        // Getting some weird issue in Win 10 where suddenly it wouldn't make the vaults dir on init
        if !self.vaults_dir.exists() {
            std::fs::create_dir_all(&self.vaults_dir)?;
        }

        let device = self.device_id;
        let vault = Vault {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: Uuid::new_v4(),
            name: name,
            color,
            // The creator is the first writer of both metadata fields.
            clock: VersionVector::from_version(device, 1),
            name_clock: FieldVersion::new(device, 1),
            color_clock: FieldVersion::new(device, 1),
            items: Vec::new(),
            secrets: Vec::new(),
            duplicate_dismissals: Vec::new(),
        };

        let salt = generate_salt();
        let master_key = derive_master_key(master_password, secret_key, &salt)?;

        self.unlocked_vaults.insert(
            vault.id,
            UnlockedVault {
                vault: vault.clone(),
                master_key,
                salt,
                dirty: false,
                // The initial save writes the file; until then it must not
                // exist.
                fingerprint: None,
            },
        );

        // Initial save (uses cached key — no re-derivation)
        self.save_vault(vault.id)?;

        Ok(vault)
    }

    /// Lock a specific vault (drops from RAM, zeroizes the master key).
    /// If vault_id is None, lock ALL vaults.
    pub fn lock_vault(&mut self, vault_id: Option<VaultId>) {
        if let Some(id) = vault_id {
            // Removing from HashMap drops UnlockedVault, which drops
            // MasterKey, which zeroizes the key bytes.
            self.unlocked_vaults.remove(&id);
        } else {
            self.unlocked_vaults.clear();
        }
    }

    /// Load vault file from disk, derive master key (Argon2id — expensive),
    /// cache it, and decrypt the vault contents.
    ///
    /// Pre-versioning (v1) accounts-only files are migrated here: the raw
    /// file is backed up to `<id>.vault.OLD`, the accounts are transformed
    /// into items, and the vault is marked dirty so the next save persists
    /// the versioned format.
    pub fn unlock_vault(
        &mut self,
        vault_id: VaultId,
        master_password: &str,
        secret_key: &[u8],
    ) -> Result<Vault, VaultError> {
        let path = self.vault_path(vault_id);

        if !path.exists() {
            return Err(VaultError::NotFound(vault_id.to_string()));
        }

        let encrypted_string = std::fs::read_to_string(&path)?;
        let file_fingerprint = fingerprint(encrypted_string.as_bytes());

        // Extract salt from the file so we can derive the key
        let salt = crate::crypto::extract_salt(&encrypted_string)?;

        // Derive master key (expensive — this is the only Argon2id call
        // for this vault until it's locked again)
        let master_key = derive_master_key(master_password, secret_key, &salt)?;

        // Decrypt using the freshly derived key
        let (json_bytes, salt_from_payload) = decrypt_with_key(&master_key, &encrypted_string)?;
        debug_assert_eq!(salt, salt_from_payload);

        // Parse — new format first, legacy accounts-only fallback for
        // anything written before schema versioning.
        let (mut vault, migrated) = match serde_json::from_slice::<Vault>(&json_bytes) {
            Ok(vault) => {
                // Versioned shape, but an outdated stamp (or no stamp at
                // all): re-stamp so the save brings it to the current format.
                let migrated = vault.schema_version < CURRENT_SCHEMA_VERSION;
                (vault, migrated)
            }
            Err(_) => {
                // Either a pre-versioning accounts-only file or corrupt
                // content. When neither parse succeeds we surface the
                // legacy parse error; both describe the same corrupted
                // payload, and the original file is left untouched.
                let legacy: LegacyVault = serde_json::from_slice(&json_bytes)?;
                (legacy.into_versioned(self.device_id), true)
            }
        };

        if migrated {
            // v2 (and v1) content has no per-field or lifecycle clocks.
            // Stamp it with *this* Device at counter 1, so two Devices that
            // migrate the same file are honestly concurrent.
            vault.stamp_migrated_items(self.device_id);
            vault.schema_version = CURRENT_SCHEMA_VERSION;

            // Back up the unmodified encrypted file before any save can
            // persist the transformed format. The on-disk file itself is
            // untouched until then, so the backup is the rollback path.
            // Any previous backup is overwritten; nothing deletes it.
            self.backup_vault_file(vault_id)?;
        }

        self.unlocked_vaults.insert(
            vault.id,
            UnlockedVault {
                vault: vault.clone(),
                master_key,
                salt,
                dirty: migrated,
                // Fingerprint the bytes as they are on disk right now, even
                // when the in-memory vault has been migrated: the first save
                // must still detect a file that changed after this unlock.
                fingerprint: Some(file_fingerprint),
            },
        );

        Ok(vault)
    }

    /// Copy the current encrypted vault file to its `.vault.OLD` backup,
    /// overwriting any previous backup.
    fn backup_vault_file(&self, vault_id: VaultId) -> Result<(), VaultError> {
        let path = self.vault_path(vault_id);
        let backup = self.vault_backup_path(vault_id);
        std::fs::copy(&path, backup)?;
        Ok(())
    }

    /// Save a specific vault to disk using its cached master key.
    /// This is FAST. no Argon2id, just AES-GCM + disk write.
    fn save_vault(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        // Borrow immutably to read data needed for encryption
        let (_, encrypted) = {
            let uv = self
                .unlocked_vaults
                .get(&vault_id)
                .ok_or(VaultError::VaultLocked)?;

            // Compact serialization (faster + smaller than to_vec_pretty)
            let json_bytes = serde_json::to_vec(&uv.vault)?;

            let encrypted = encrypt_with_key(&uv.master_key, &uv.salt, &json_bytes)?;
            (json_bytes, encrypted)
        };
        // Immutable borrow ends here — NLL allows mutable borrow below

        let path = self.vault_path(vault_id);
        let expected = self
            .unlocked_vaults
            .get(&vault_id)
            .ok_or(VaultError::VaultLocked)?
            .fingerprint;

        ensure_file_unchanged(&path, expected, vault_id)?;

        // Atomic save: temp file + fsync + rename, so a crash leaves the
        // previous vault readable.
        write_atomic(&path, encrypted.as_bytes())?;

        // Clear dirty flag and record the fingerprint of what we just wrote
        if let Some(uv) = self.unlocked_vaults.get_mut(&vault_id) {
            uv.fingerprint = Some(fingerprint(encrypted.as_bytes()));
            uv.dirty = false;
        }

        Ok(())
    }

    /// Explicitly flush a specific vault to disk if it's dirty.
    /// Returns Ok(()) even if the vault wasn't dirty.
    pub fn flush_vault(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        let is_dirty = self
            .unlocked_vaults
            .get(&vault_id)
            .map(|uv| uv.dirty)
            .unwrap_or(false);

        if is_dirty {
            self.save_vault(vault_id)?;
        }
        Ok(())
    }

    /// Flush ALL dirty vaults to disk. Useful before closing / backgrounding.
    pub fn flush_all(&mut self) -> Result<(), VaultError> {
        let dirty_ids: Vec<VaultId> = self
            .unlocked_vaults
            .iter()
            .filter(|(_, uv)| uv.dirty)
            .map(|(id, _)| *id)
            .collect();

        for id in dirty_ids {
            self.save_vault(id)?;
        }

        Ok(())
    }

    /// Check if a vault has unsaved changes
    pub fn is_dirty(&self, vault_id: VaultId) -> bool {
        self.unlocked_vaults
            .get(&vault_id)
            .map(|uv| uv.dirty)
            .unwrap_or(false)
    }

    /// Update a specific vault's metadata. Saves immediately if autosave
    /// is enabled; otherwise marks dirty.
    ///
    /// Name and colour are independent fields with their own last-writer
    /// versions, so a rename and a recolour made on different Devices merge
    /// instead of one silently discarding the other.
    pub fn update_vault(
        &mut self,
        vault_id: VaultId,
        name: Option<String>,
        color: Option<String>,
    ) -> Result<(), VaultError> {
        {
            let device = self.device_id;
            let vault = self.get_vault_mut(vault_id)?;
            if let Some(n) = name {
                let counter = vault.clock.increment(device);
                vault.name = n;
                vault.name_clock = FieldVersion::new(device, counter);
            }
            if let Some(c) = color {
                let counter = vault.clock.increment(device);
                vault.color = c;
                vault.color_clock = FieldVersion::new(device, counter);
            }
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Remove this Device's copy of a Vault: lock it and delete the file.
    ///
    /// This is the "remove local copy" removal operation and nothing more: it
    /// is **not** a deletion and never leaves a Tombstone, because it means
    /// "I do not want it here", not "it is gone". The caller is responsible
    /// for taking the Vault out of this Device's Sync Set; otherwise the next
    /// sync would simply install it again from a peer.
    pub fn remove_local_copy(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        if self.is_vault_unlocked(vault_id) {
            self.lock_vault(Some(vault_id));
        }

        let path = self.vault_path(vault_id);

        std::fs::remove_file(path).map_err(VaultError::Io)?;

        Ok(())
    }

    /// Move this Device's copy of a Vault to local trash
    /// (`<vault_id>.vault.deleted`), keeping the encrypted file recoverable.
    ///
    /// This is the local half of "delete everywhere": the Vault Registry's
    /// Tombstone is what travels, and every Device calls this when it learns
    /// it. It is idempotent, because a deletion can arrive twice (or from two
    /// Devices), and it never fails because the file is already gone.
    /// Returns whether a live file was moved.
    pub(crate) fn move_to_trash(&mut self, vault_id: VaultId) -> Result<bool, VaultError> {
        if self.is_vault_unlocked(vault_id) {
            self.lock_vault(Some(vault_id));
        }

        let path = self.vault_path(vault_id);
        if !path.is_file() {
            return Ok(false);
        }

        let trash = self.vault_trash_path(vault_id);
        // A previous trash is superseded; Windows rename refuses to overwrite.
        if trash.exists() {
            std::fs::remove_file(&trash)?;
        }
        std::fs::rename(&path, &trash)?;
        Ok(true)
    }

    /// Delete an item of any kind and its secret entry, if any. The item is
    /// retained as a Tombstone — identity and clock only — so the deletion
    /// can travel to other Devices instead of the item reappearing on sync.
    ///
    /// Deleting an already-deleted item succeeds quietly: a deletion can
    /// reach a Device twice (a retry, or the same deletion made on two
    /// Devices), and treating the second one as an error would make sync
    /// order-dependent. A move record is *not* a deletion, so deleting the
    /// identity of an item that moved away is still `ItemNotFound`.
    pub fn delete_item(&mut self, vault_id: VaultId, item_id: ItemId) -> Result<(), VaultError> {
        {
            let vault = self.get_vault(vault_id)?;
            let has_live_item = vault
                .items
                .iter()
                .any(|i| i.id == item_id && !i.is_content_free());
            let already_deleted = vault.items.iter().any(|i| i.id == item_id && i.is_deleted());
            if !has_live_item && already_deleted {
                return Ok(());
            }
        }

        {
            let device = self.device_id;
            let vault = self.get_vault_mut(vault_id)?;
            let index = vault
                .items
                .iter()
                .position(|i| i.id == item_id && !i.is_content_free())
                .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;

            let mut item = vault.items.remove(index);
            item.clock.increment(device);

            vault.items.push(VaultItem::tombstone(
                item.id,
                item.vault_id,
                item.clock,
            ));
            vault.secrets.retain(|s| s.item_id != item_id);
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Create a new item of any kind, together with its secret fields, if
    /// the kind stores any (account items carry their initial password as
    /// the `password` field key). Secret keys are validated against the
    /// kind: unknown keys and secret-less kinds are rejected.
    pub fn add_item(
        &mut self,
        vault_id: VaultId,
        display_name: Option<String>,
        icon: Option<String>,
        data: NewItemData,
        secrets: Option<Vec<SecretInput>>,
    ) -> Result<SafeItem, VaultError> {
        let kind = match data {
            NewItemData::Account(mut account) => {
                // Account email follows the shared text-field rule:
                // empty strings mean 'not set'
                account.email = normalize(account.email);
                ItemKind::Account(account)
            }
            NewItemData::Note(note) => ItemKind::Note(note),
            NewItemData::Card(card) => {
                if !(1..=12).contains(&card.expiry_month) {
                    return Err(VaultError::Validation(format!(
                        "Card expiry month must be between 1 and 12, got {}",
                        card.expiry_month
                    )));
                }
                ItemKind::Card(card)
            }
            NewItemData::Identity(identity) => ItemKind::Identity(identity),
        };

        // A new item starts at this Device's next lifecycle counter; every
        // field it carries shares that counter as its last-writer version.
        let device = self.device_id;
        let mut item_clock = VersionVector::new();
        let counter = item_clock.increment(device);
        let version = FieldVersion::new(device, counter);

        // Validate the secret inputs against the kind before mutating anything
        let secret_fields: Vec<crate::SecretField> = {
            let allowed = allowed_secret_keys(&kind);
            match &secrets {
                Some(inputs) if !inputs.is_empty() => {
                    if allowed.is_empty() {
                        return Err(VaultError::Validation(
                            "This kind of item has no secret fields".to_string(),
                        ));
                    }

                    let mut fields = Vec::with_capacity(inputs.len());
                    for input in inputs {
                        if !allowed.contains(&input.key.as_str()) {
                            return Err(VaultError::Validation(format!(
                                "Unknown secret field '{}' for this kind of item",
                                input.key
                            )));
                        }
                        fields.push(crate::SecretField {
                            key: input.key.clone(),
                            value: input.value.clone(),
                            clock: version,
                        });
                    }
                    fields
                }
                // Secret-less kinds (and omitted input) store no entry
                _ => Vec::new(),
            }
        };

        let now = Utc::now();
        let mut item = VaultItem {
            id: Uuid::new_v4(),
            vault_id,
            display_name: normalize(display_name),
            favourite: false,
            tags: Vec::new(),
            icon,
            color: String::new(),
            created_at: now,
            updated_at: now,
            clock: item_clock,
            field_clocks: Default::default(),
            deleted: false,
            moved_to: None,
            kind: Some(kind),
        };
        item.stamp_all_fields(device, counter);
        for field in &secret_fields {
            item.record_field(&field.key, version);
        }

        {
            let vault = self.get_vault_mut(vault_id)?;
            vault.items.push(item.clone());
            if !secret_fields.is_empty() {
                vault.secrets.push(ItemSecrets {
                    item_id: item.id,
                    fields: secret_fields,
                });
            }
        }

        self.maybe_save(vault_id)?;

        self.safe_view_of(vault_id, item.id)
    }

    /// Builds the safe view of an item against its vault's current state
    /// (card items derive last4 from the vault's secret store). Tombstones
    /// have no safe view and are treated as missing.
    fn safe_view_of(&self, vault_id: VaultId, item_id: ItemId) -> Result<SafeItem, VaultError> {
        let vault = self.get_vault(vault_id)?;
        let item = vault
            .items
            .iter()
            .find(|i| i.id == item_id && !i.is_content_free())
            .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;
        item_to_safe(item, &vault.secrets).ok_or_else(|| {
            VaultError::ItemNotFound(item_id.to_string())
        })
    }

    /// Update an item's metadata, and for supported kinds its kind data.
    /// Kind data may not change an item's kind.
    ///
    /// All validation happens against an immutable view first. A rejected
    /// update therefore neither advances the lifecycle clock nor marks the
    /// vault dirty. Kind-data updates stamp only the fields that actually
    /// changed, so per-field clocks describe the write they name.
    pub fn update_item(
        &mut self,
        vault_id: VaultId,
        item_id: ItemId,
        display_name: Option<String>,
        icon: Option<String>,
        favourite: Option<bool>,
        tags: Option<Vec<String>>,
        color: Option<String>,
        mut data: Option<NewItemData>,
    ) -> Result<SafeItem, VaultError> {
        // Account email follows the shared text-field rule before either
        // comparison or assignment sees it.
        if let Some(NewItemData::Account(account)) = data.as_mut() {
            account.email = normalize(account.email.take());
        }

        let has_metadata_change = display_name.is_some()
            || icon.is_some()
            || favourite.is_some()
            || tags.is_some()
            || color.is_some();

        // Validate before touching the vault: no clock bump, no dirty flag.
        let changed_fields = {
            let vault = self.get_vault(vault_id)?;
            let item = vault
                .items
                .iter()
                .find(|i| i.id == item_id && !i.is_content_free())
                .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;

            match &data {
                Some(new_data) => {
                    let kind = item
                        .kind
                        .as_ref()
                        .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;
                    validate_kind_data(kind, new_data)?;
                    changed_kind_fields(kind, new_data)
                }
                None => Vec::new(),
            }
        };

        // An update that changes nothing is not a write at all.
        if !has_metadata_change && changed_fields.is_empty() {
            return self.safe_view_of(vault_id, item_id);
        }

        {
            let device = self.device_id;
            let vault = self.get_vault_mut(vault_id)?;

            let item = vault
                .items
                .iter_mut()
                .find(|i| i.id == item_id && !i.is_content_free())
                .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;

            let counter = item.clock.increment(device);
            let version = FieldVersion::new(device, counter);

            if let Some(fav) = favourite {
                item.favourite = fav;
                item.record_field("favourite", version);
            }
            if let Some(t) = tags {
                item.tags = t;
                item.record_field("tags", version);
            }
            if let Some(c) = color {
                item.color = c;
                item.record_field("color", version);
            }
            if let Some(name) = display_name {
                item.display_name = normalize(Some(name));
                item.record_field("display_name", version);
            }
            if let Some(icon) = icon {
                item.icon = normalize(Some(icon));
                item.record_field("icon", version);
            }

            if let Some(data) = data {
                match (item.kind.as_mut(), data) {
                    (Some(ItemKind::Account(current)), NewItemData::Account(new)) => {
                        *current = new
                    }
                    (Some(ItemKind::Note(current)), NewItemData::Note(new)) => *current = new,
                    (Some(ItemKind::Identity(current)), NewItemData::Identity(new)) => {
                        *current = new
                    }
                    (Some(ItemKind::Card(current)), NewItemData::Card(new)) => *current = new,
                    // Validated above; the item cannot lose its kind here.
                    _ => unreachable!("kind data was validated before mutation"),
                }

                for key in changed_fields {
                    item.record_field(key, version);
                }
            }

            item.updated_at = Utc::now();
        };

        // Card items pull their last4 from the secret store, so the safe
        // view is built after the mutable section against fresh vault state
        let updated = self.safe_view_of(vault_id, item_id)?;

        self.maybe_save(vault_id)?;
        Ok(updated)
    }

    /// Retrieve a specific item of any sendable kind by id (all unlocked vaults).
    pub fn get_item_by_id(&self, item_id: ItemId) -> Result<SafeItem, VaultError> {
        for uv in self.unlocked_vaults.values() {
            if let Some(item) = uv
                .vault
                .items
                .iter()
                .find(|i| i.id == item_id && !i.is_content_free())
            {
                return item_to_safe(item, &uv.vault.secrets)
                    .ok_or_else(|| VaultError::ItemNotFound(item_id.to_string()));
            }
        }

        Err(VaultError::ItemNotFound(item_id.to_string()))
    }

    /// Move an item of any kind (and its secret entry, if any) to another
    /// vault as one atomic mutation.
    ///
    /// The item keeps its identity: move is a lifecycle event on the same
    /// UUID, not delete-plus-create. Because the lifecycle clock is bumped,
    /// the move is causally visible to a later merge, which can tell it
    /// apart from an unrelated item that merely happens to share a name.
    pub fn move_item(
        &mut self,
        vault_id: VaultId,
        item_id: ItemId,
        target_vault_id: VaultId,
    ) -> Result<SafeItem, VaultError> {
        if vault_id == target_vault_id {
            return Err(VaultError::Validation(
                "Item is already in this vault".to_string(),
            ));
        }

        // Both vaults must be unlocked before anything is mutated
        self.get_vault(vault_id)?;
        self.get_vault(target_vault_id)?;

        // Remove the item and its secret entry from the source vault
        let (item, mut secret) = {
            let device = self.device_id;
            let source = self.get_vault_mut(vault_id)?;
            let idx = source
                .items
                .iter()
                .position(|i| i.id == item_id && !i.is_content_free())
                .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;
            let mut item = source.items.remove(idx);

            let secret = source
                .secrets
                .iter()
                .position(|s| s.item_id == item_id)
                .map(|s_idx| source.secrets.remove(s_idx));

            // A move is a lifecycle event on the same identity: preserve the
            // id and record the event by advancing the item's clock.
            item.clock.increment(device);

            // Leave a causal record in the source vault naming the
            // destination. Without it a peer still holding this copy would
            // see a live item against absence and resurrect a duplicate in
            // both vaults. It is not a Tombstone: moving back must be able to
            // win over it.
            source.items.retain(|existing| existing.id != item.id);
            source.items.push(VaultItem::move_record(
                item.id,
                vault_id,
                item.clock.clone(),
                target_vault_id,
            ));

            item.vault_id = target_vault_id;
            item.updated_at = Utc::now();

            (item, secret)
        };

        // Insert into the target vault with the secret entry remapped. An old
        // move record for this identity may already be here from a previous
        // move out; the live item replaces it, so one identity never has two
        // entries in one vault.
        {
            let target = self.get_vault_mut(target_vault_id)?;
            target.items.retain(|existing| existing.id != item.id);
            target
                .secrets
                .retain(|existing| existing.item_id != item.id);
            if let Some(secret) = secret.as_mut() {
                secret.item_id = item.id;
                target.secrets.push(secret.clone());
            }
            target.items.push(item.clone());
        }

        self.maybe_save(vault_id)?;
        self.maybe_save(target_vault_id)?;

        // Card items pull their last4 from the (remapped) secret entry
        self.safe_view_of(target_vault_id, item.id)
    }

    /// Retrieves items of all kinds across one or all vaults, applying filters.
    pub fn get_all_items(&self, filter: ItemFilter) -> Result<Vec<SafeItem>, VaultError> {
        let mut results: Vec<SafeItem> = Vec::new();

        let vaults_to_scan: Vec<&Vault> = if let Some(id) = filter.vault_id {
            vec![self.get_vault(id)?]
        } else {
            self.unlocked_vaults.values().map(|uv| &uv.vault).collect()
        };

        let query = filter.search_query.map(|q| q.to_lowercase());

        for vault in vaults_to_scan {
            for item in &vault.items {
                // Tombstones are retained for sync but never listed.
                let Some(view) = item_to_safe(item, &vault.secrets) else {
                    continue;
                };

                if let Some(true) = filter.favourite_only {
                    if !item.favourite {
                        continue;
                    }
                }

                if let Some(ref filter_tags) = filter.tags {
                    if !filter_tags.is_empty() {
                        let has_match = filter_tags.iter().any(|ft| item.tags.contains(ft));
                        if !has_match {
                            continue;
                        }
                    }
                }

                if let Some(ref q) = query {
                    let matches_search =
                        item.search_text().iter().any(|text| text.contains(q));
                    if !matches_search {
                        continue;
                    }
                }

                results.push(view);
            }
        }

        results.sort_by(|a, b| {
            let updated = |i: &SafeItem| match i {
                SafeItem::Account(v) => v.updated_at,
                SafeItem::Card(v) => v.updated_at,
                SafeItem::Note(v) => v.updated_at,
                SafeItem::Identity(v) => v.updated_at,
            };
            updated(b).cmp(&updated(a))
        });

        Ok(results)
    }

    /// Returns a specific secret field value for an item. An unset field
    /// resolves to None; unknown field keys, secret-less kinds and failed
    /// lookups are real errors, so callers can distinguish 'not set' from
    /// 'fetch failed'.
    pub fn get_item_secret(
        &self,
        vault_id: VaultId,
        item_id: ItemId,
        field_key: &str,
    ) -> Result<Option<String>, VaultError> {
        let vault = self.get_vault(vault_id)?;

        let item = vault
            .items
            .iter()
            .find(|i| i.id == item_id && !i.is_content_free())
            .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;

        let allowed = allowed_secret_keys(
            item.kind
                .as_ref()
                .expect("a live item always carries a kind"),
        );
        if allowed.is_empty() {
            return Err(VaultError::Validation(
                "This kind of item has no secret fields".to_string(),
            ));
        }
        if !allowed.contains(&field_key) {
            return Err(VaultError::Validation(format!(
                "Unknown secret field '{field_key}' for this kind of item"
            )));
        }

        // No secrets entry at all, or an entry without this field, simply
        // means the field is unset
        let entry = vault.secrets.iter().find(|s| s.item_id == item_id);
        Ok(entry.and_then(|s| s.get(field_key)).map(str::to_string))
    }

    /// Sets or clears a specific secret field of an item. Clearing (a `None`
    /// value) removes the field, and an entry left with no fields is dropped.
    pub fn update_item_secret(
        &mut self,
        vault_id: VaultId,
        item_id: ItemId,
        field_key: &str,
        value: Option<String>,
    ) -> Result<(), VaultError> {
        // Validate the item's kind and field key before touching the store
        let is_noop = {
            let vault = self.get_vault_mut(vault_id)?;
            let item = vault
                .items
                .iter()
                .find(|i| i.id == item_id && !i.is_content_free())
                .ok_or(VaultError::ItemNotFound(item_id.to_string()))?;

            let allowed = allowed_secret_keys(
                item.kind
                    .as_ref()
                    .expect("a live item always carries a kind"),
            );
            if allowed.is_empty() {
                return Err(VaultError::Validation(
                    "This kind of item has no secret fields".to_string(),
                ));
            }
            if !allowed.contains(&field_key) {
                return Err(VaultError::Validation(format!(
                    "Unknown secret field '{field_key}' for this kind of item",
                )));
            }

            // Clearing a field the item doesn't even have is a no-op
            value.is_none()
                && !vault
                    .secrets
                    .iter()
                    .any(|s| s.item_id == item_id && s.get(field_key).is_some())
        };

        if is_noop {
            return Ok(());
        }

        {
            let device = self.device_id;
            let vault = self.get_vault_mut(vault_id)?;
            let item = vault
                .items
                .iter_mut()
                .find(|i| i.id == item_id && !i.is_content_free())
                .expect("the item was just validated");

            // A secret is a field like any other: bump the item's lifecycle
            // clock and record the same version against the field key.
            let counter = item.clock.increment(device);
            let version = FieldVersion::new(device, counter);
            item.updated_at = Utc::now();
            item.record_field(field_key, version);

            let entry_pos = vault.secrets.iter().position(|s| s.item_id == item_id);

            match (entry_pos, value) {
                (Some(pos), Some(value)) => vault.secrets[pos].set(field_key, value, version),
                (Some(pos), None) => {
                    vault.secrets[pos].remove(field_key);
                    // Drop emptied entries to keep the store tidy
                    if vault.secrets[pos].fields.is_empty() {
                        vault.secrets.remove(pos);
                    }
                }
                (None, Some(value)) => vault
                    .secrets
                    .push(ItemSecrets::new(item_id, field_key, value, version)),
                (None, None) => unreachable!("no-op clears return above"),
            }
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Get the strength of an item's password-style secret (the `password`
    /// field key; kind-aware through the single per-field fetch surface)
    pub fn get_item_password_strength(
        &self,
        vault_id: VaultId,
        item_id: ItemId,
    ) -> Result<Entropy, VaultError> {
        match self.get_item_secret(vault_id, item_id, SECRET_FIELD_PASSWORD)? {
            // Passwordless items surface as a failed lookup (the frontend
            // wrapper turns that into 'no strength'), matching the
            // pre-rename behaviour
            Some(pw) => Ok(zxcvbn(&pw, &[])),
            None => Err(VaultError::Validation("Item has no password".to_string())),
        }
    }

    /// List the Duplicates this vault currently suggests. Purely a read: a
    /// suggestion is never applied, and resolving one always takes an
    /// explicit call to [`VaultManager::resolve_duplicate`].
    pub fn duplicate_suggestions(
        &self,
        vault_id: VaultId,
    ) -> Result<Vec<crate::DuplicateSuggestion>, VaultError> {
        let vault = self.get_vault(vault_id)?;
        Ok(crate::duplicates::suggest_duplicates(vault))
    }

    /// Apply the User's decision about a suggested pair. Keeping both records
    /// a dismissal in the vault, so the decision syncs; keeping one identity
    /// or Combining tombstones the other.
    pub fn resolve_duplicate(
        &mut self,
        vault_id: VaultId,
        first: ItemId,
        second: ItemId,
        resolution: crate::DuplicateResolution,
    ) -> Result<crate::DuplicateOutcome, VaultError> {
        // Mark the vault dirty only after the resolution has been accepted, so
        // a rejected Combine neither advances a clock nor triggers a save.
        let device = self.device_id;
        let outcome = {
            let unlocked = self
                .unlocked_vaults
                .get_mut(&vault_id)
                .ok_or(VaultError::VaultLocked)?;
            let outcome = crate::duplicates::resolve_duplicate(
                &mut unlocked.vault,
                first,
                second,
                resolution,
                device,
            )?;
            unlocked.dirty = true;
            outcome
        };

        self.maybe_save(vault_id)?;
        Ok(outcome)
    }

    // ============ Sync support ============

    /// Whether this Device holds a vault file for `vault_id`. Used by the
    /// sync engine to decide what it can offer; it never offers a Vault it
    /// does not have, and `.OLD` backups are invisible here.
    pub(crate) fn has_vault_file(&self, vault_id: VaultId) -> bool {
        self.vault_path(vault_id).is_file()
    }

    /// The encrypted bytes exactly as they sit on disk. This is what travels
    /// during a sync: no decryption happens on the sending side.
    pub(crate) fn read_encrypted_vault(&self, vault_id: VaultId) -> Result<Vec<u8>, VaultError> {
        std::fs::read(self.vault_path(vault_id)).map_err(Into::into)
    }

    /// Decrypt a peer's encrypted vault file with the credentials resident in
    /// this session. Nothing is written: the caller merges it first.
    pub(crate) fn decrypt_vault_bytes(
        &self,
        bytes: &[u8],
        master_password: &str,
        secret_key: &[u8],
    ) -> Result<Vault, VaultError> {
        let encoded = std::str::from_utf8(bytes)
            .map_err(|_| VaultError::Decryption("vault blob is not valid UTF-8".into()))?;
        let salt = crate::crypto::extract_salt(encoded)?;
        let master_key = derive_master_key(master_password, secret_key, &salt)?;
        let (json_bytes, _) = decrypt_with_key(&master_key, encoded)?;
        serde_json::from_slice(&json_bytes).map_err(Into::into)
    }

    /// Install a vault this Device did not have: re-encrypt it under a locally
    /// generated salt with the same User credentials and write it atomically.
    ///
    /// The file is written *before* the vault is published in memory. A save
    /// failure therefore leaves no half-installed vault that later reads
    /// report as unlocked but that has no file on disk.
    pub(crate) fn install_vault(
        &mut self,
        vault: Vault,
        master_password: &str,
        secret_key: &[u8],
    ) -> Result<(), VaultError> {
        let path = self.vault_path(vault.id);
        if path.exists() {
            return Err(VaultError::ExternalChange(vault.id.to_string()));
        }

        let vault_id = vault.id;
        let salt = generate_salt();
        let master_key = derive_master_key(master_password, secret_key, &salt)?;
        let json_bytes = serde_json::to_vec(&vault)?;
        let encrypted = encrypt_with_key(&master_key, &salt, &json_bytes)?;

        ensure_file_unchanged(&path, None, vault_id)?;
        write_atomic(&path, encrypted.as_bytes())?;

        self.unlocked_vaults.insert(
            vault_id,
            UnlockedVault {
                vault,
                master_key,
                salt,
                dirty: false,
                fingerprint: Some(fingerprint(encrypted.as_bytes())),
            },
        );

        Ok(())
    }

    /// Replace an unlocked vault with a merged result and save it atomically.
    pub(crate) fn replace_vault(&mut self, vault: Vault) -> Result<(), VaultError> {
        let vault_id = vault.id;
        let unlocked = self
            .unlocked_vaults
            .get_mut(&vault_id)
            .ok_or(VaultError::VaultLocked)?;
        unlocked.vault = vault;
        unlocked.dirty = true;
        self.save_vault(vault_id)
    }

    /// Apply the User's explicit choice for one contested field. The value is
    /// stamped as a fresh local write so it beats both conflicting versions
    /// on the next sync. Returns whether the conflict still described a field
    /// that exists.
    ///
    /// `expected_local` is the local field version the User was shown when the
    /// sync surfaced the Conflict. If the local field has been written since
    /// then, the candidate is stale and is refused: applying it would silently
    /// clobber newer work. A refusal is a distinct error, so the caller can
    /// re-sync and re-prompt instead of pretending the choice was applied.
    pub(crate) fn resolve_conflict_field(
        &mut self,
        vault_id: VaultId,
        scope: &crate::ConflictScope,
        field_key: &str,
        is_secret: bool,
        expected_local: Option<FieldVersion>,
        value: Option<&crate::merge::FieldValue>,
    ) -> Result<bool, VaultError> {
        let device = self.device_id;
        let applied = {
            let unlocked = self
                .unlocked_vaults
                .get_mut(&vault_id)
                .ok_or(VaultError::VaultLocked)?;
            let current = current_field_version(&unlocked.vault, scope, field_key);
            if current != expected_local {
                return Err(VaultError::SyncConflictResolution(format!(
                    "{field_key} changed on this Device after the sync surfaced it; sync again before choosing"
                )));
            }
            let applied = crate::merge::apply_conflict_value(
                &mut unlocked.vault,
                scope,
                field_key,
                is_secret,
                value,
                device,
            );
            if applied {
                unlocked.dirty = true;
            }
            applied
        };

        if applied {
            self.maybe_save(vault_id)?;
        }
        Ok(applied)
    }

    /// Vault files present on disk but absent from the shared Vault Registry.
    /// These are never synced until the User explicitly adopts them.
    pub(crate) fn unregistered_vault_files(
        &self,
        registry: &crate::VaultRegistry,
    ) -> Result<Vec<VaultId>, VaultError> {
        let mut ids = Vec::new();
        if !self.vaults_dir.exists() {
            return Ok(ids);
        }

        for entry in read_dir(&self.vaults_dir)? {
            let path = entry?.path();
            // `.vault.OLD` is a migration backup and must never be adopted or
            // transferred. Exact extension matching keeps it out.
            if path.extension().and_then(|ext| ext.to_str()) != Some("vault") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(vault_id) = Uuid::parse_str(stem) else {
                continue;
            };
            if !registry.contains(&vault_id) {
                ids.push(vault_id);
            }
        }

        ids.sort();
        Ok(ids)
    }

    /// Save immediately if autosave is on. Otherwise just leave the dirty
    /// flag set (caller is responsible for flushing).
    fn maybe_save(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        if self.autosave {
            self.save_vault(vault_id)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        card_number_last4, ClockOrder, SECRET_FIELD_CARD_NUMBER, SECRET_FIELD_PIN,
        SECRET_FIELD_SECURITY_CODE,
    };

    fn setup() -> PathBuf {
        let unique_id = Uuid::new_v4();
        let temp_dir = std::env::temp_dir().join(format!("vault_manager_test_{}", unique_id));
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).unwrap();
        temp_dir
    }

    const MASTER_PW: &str = "SuperSecret123";
    const KEY: [u8; 32] = [0u8; 32];

    // ============ Legacy (v1) vault file fixtures ============

    use chrono::{DateTime, Utc};

    fn fixture_time(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    /// A full-featured and a minimal legacy account, as the pre-versioning
    /// app would have written them.
    fn legacy_accounts() -> (Vec<crate::Account>, Uuid, Uuid) {
        let vault_id = Uuid::new_v4();
        let full_id = Uuid::new_v4();
        let minimal_id = Uuid::new_v4();

        let accounts = vec![
            crate::Account {
                id: full_id,
                vault_id,
                display_name: Some("Old Account".to_string()),
                username: "legacy_user".to_string(),
                email: Some("legacy@example.com".to_string()),
                favourite: true,
                tags: vec!["legacy".to_string()],
                icon: Some("github".to_string()),
                color: "#6240BF".to_string(),
                created_at: fixture_time(1_700_000_000),
                updated_at: fixture_time(1_701_000_000),
                secret: crate::AccountSecret {
                    id: Uuid::new_v4(),
                    password: "legacy_pw_full".to_string(),
                },
            },
            crate::Account {
                id: minimal_id,
                vault_id,
                display_name: None,
                username: "second".to_string(),
                email: None,
                favourite: false,
                tags: Vec::new(),
                icon: None,
                color: String::new(),
                created_at: fixture_time(1_700_000_500),
                updated_at: fixture_time(1_700_000_500),
                secret: crate::AccountSecret {
                    id: Uuid::new_v4(),
                    password: "legacy_pw_minimal".to_string(),
                },
            },
        ];

        (accounts, vault_id, full_id)
    }

    /// Writes a legacy (v1, accounts-only) vault file, exactly as versions
    /// before schema versioning did.
    fn write_legacy_vault_file(temp_dir: &PathBuf, vault_id: Uuid, accounts: &[crate::Account]) {
        let legacy = crate::LegacyVault {
            id: vault_id,
            name: "Legacy Vault".to_string(),
            color: "#6240BF".to_string(),
            accounts: accounts.to_vec(),
        };

        let json = serde_json::to_vec(&legacy).unwrap();
        let salt = [7u8; 32];
        let master_key = derive_master_key(MASTER_PW, &KEY, &salt).unwrap();
        let encrypted = encrypt_with_key(&master_key, &salt, &json).unwrap();

        let vaults_dir = temp_dir.join("vaults");
        std::fs::create_dir_all(&vaults_dir).unwrap();
        std::fs::write(vaults_dir.join(format!("{}.vault", vault_id)), encrypted).unwrap();
    }

    #[test]
    fn test_migrates_legacy_vault_on_unlock() {
        let temp_dir = setup();
        let (accounts, vault_id, full_id) = legacy_accounts();
        write_legacy_vault_file(&temp_dir, vault_id, &accounts);

        let backup_path = temp_dir
            .join("vaults")
            .join(format!("{}.vault.OLD", vault_id));

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let migrated = manager.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();

        // Vault is stamped with the current schema version
        assert_eq!(migrated.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(manager.is_dirty(vault_id), "Migrated vault must be flagged dirty");

        // Every account migrated with identical metadata
        let all = manager
            .get_all_items(ItemFilter::default())
            .unwrap();
        assert_eq!(all.len(), 2);
        for legacy in &accounts {
            let view = match all
                .iter()
                .find(|s| matches!(s, SafeItem::Account(a) if a.id == legacy.id))
                .unwrap()
            {
                SafeItem::Account(a) => a.clone(),
                other => panic!("expected migrated account view, got {other:?}"),
            };
            assert_eq!(view.username, legacy.username);
            assert_eq!(view.email, legacy.email);
            assert_eq!(view.display_name, legacy.display_name);
            assert_eq!(view.favourite, legacy.favourite);
            assert_eq!(view.tags, legacy.tags);
            assert_eq!(view.icon, legacy.icon);
            assert_eq!(view.color, legacy.color);
            assert_eq!(view.created_at, legacy.created_at);
            assert_eq!(view.updated_at, legacy.updated_at);
        }

        // Passwords come through the existing secret-fetch path
        assert_eq!(
            manager
                .get_item_secret(vault_id, full_id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("legacy_pw_full")
        );

        // Raw unmodified file is backed up beside the vault
        assert!(backup_path.exists());
        let raw = std::fs::read(temp_dir.join("vaults").join(format!("{}.vault", vault_id))).unwrap();
        assert_eq!(
            std::fs::read(&backup_path).unwrap(),
            raw,
            "Backup must capture the pre-migration file"
        );

        // Persist the migrated format, then re-unlock: no second migration
        manager.flush_vault(vault_id).unwrap();
        assert!(!manager.is_dirty(vault_id));
        let backup_before = std::fs::read(&backup_path).unwrap();

        manager.lock_vault(Some(vault_id));
        let re_unlocked = manager.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();

        assert_eq!(re_unlocked.schema_version, CURRENT_SCHEMA_VERSION);
        assert!(!manager.is_dirty(vault_id), "v2 unlock must not re-migrate");
        assert_eq!(
            std::fs::read(&backup_path).unwrap(),
            backup_before,
            "Backup must not be rewritten on plain unlocks"
        );
        assert_eq!(
            manager
                .get_item_secret(vault_id, full_id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("legacy_pw_full")
        );
    }

    /// Encrypt arbitrary vault plaintext into a vault file, as an older
    /// writer would have.
    fn write_encrypted_vault_file(
        temp_dir: &PathBuf,
        vault_id: Uuid,
        plaintext: &[u8],
    ) -> PathBuf {
        let salt = [7u8; 32];
        let master_key = derive_master_key(MASTER_PW, &KEY, &salt).unwrap();
        let encrypted = encrypt_with_key(&master_key, &salt, plaintext).unwrap();

        let vaults_dir = temp_dir.join("vaults");
        std::fs::create_dir_all(&vaults_dir).unwrap();
        let path = vaults_dir.join(format!("{}.vault", vault_id));
        std::fs::write(&path, encrypted).unwrap();
        path
    }

    /// Writes a v2 vault (schema version 2, no clock fields anywhere).
    fn write_v2_vault_file(
        temp_dir: &PathBuf,
        vault_id: Uuid,
        item_id: Uuid,
    ) -> PathBuf {
        let json = serde_json::json!({
            "schema_version": 2,
            "id": vault_id.to_string(),
            "name": "V2 Vault",
            "color": "#6240BF",
            "items": [{
                "id": item_id.to_string(),
                "vault_id": vault_id.to_string(),
                "display_name": "Old login",
                "favourite": true,
                "tags": ["legacy"],
                "icon": "github",
                "color": "#6240BF",
                "created_at": "2023-11-14T22:13:20Z",
                "updated_at": "2023-11-14T22:13:20Z",
                "kind": "account",
                "username": "octocat",
                "email": "octo@example.com"
            }],
            "secrets": [{
                "item_id": item_id.to_string(),
                "fields": [{"key": "password", "value": "v2_password"}]
            }]
        });

        write_encrypted_vault_file(temp_dir, vault_id, &serde_json::to_vec(&json).unwrap())
    }

    #[test]
    fn test_migrates_v2_vault_to_v3_with_byte_identical_backup() {
        let temp_dir = setup();
        let vault_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();
        let vault_path = write_v2_vault_file(&temp_dir, vault_id, item_id);
        let backup_path = temp_dir
            .join("vaults")
            .join(format!("{}.vault.OLD", vault_id));
        let before = std::fs::read(&vault_path).unwrap();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let migrated = manager.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();

        assert_eq!(CURRENT_SCHEMA_VERSION, 3);
        assert_eq!(migrated.schema_version, 3);
        assert!(manager.is_dirty(vault_id), "migration must mark the vault dirty");
        assert_eq!(
            std::fs::read(&backup_path).unwrap(),
            before,
            "the rollback copy must be byte-identical to the pre-migration file"
        );

        // The migrated content is stamped by this installation's Device.
        let device = manager.device_id();
        assert_eq!(migrated.clock.get(&device), 1);
        assert_eq!(migrated.name_clock, FieldVersion::new(device, 1));
        assert_eq!(migrated.color_clock, FieldVersion::new(device, 1));
        let item = &migrated.items[0];
        assert_eq!(item.clock.get(&device), 1);
        assert_eq!(
            item.field_clocks.get("username"),
            Some(&FieldVersion::new(device, 1))
        );
        assert_eq!(
            item.field_clocks.get("display_name"),
            Some(&FieldVersion::new(device, 1))
        );
        assert_eq!(
            migrated.secrets[0].fields[0].clock,
            FieldVersion::new(device, 1)
        );

        // The v3 save changes the vault file but never the backup.
        manager.flush_vault(vault_id).unwrap();
        assert_ne!(std::fs::read(&vault_path).unwrap(), before);

        manager.lock_vault(Some(vault_id));
        let re_unlocked = manager.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();
        assert_eq!(re_unlocked.schema_version, 3);
        assert!(!manager.is_dirty(vault_id), "a v3 unlock must not re-migrate");
        assert_eq!(
            std::fs::read(&backup_path).unwrap(),
            before,
            "re-unlocking a v3 vault must not touch the backup"
        );
        assert_eq!(
            manager
                .get_item_secret(vault_id, item_id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("v2_password")
        );
    }

    #[test]
    fn test_two_devices_migrating_same_v2_vault_get_concurrent_clocks() {
        let first_dir = setup();
        let second_dir = setup();
        let vault_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();

        let first_path = write_v2_vault_file(&first_dir, vault_id, item_id);
        let encrypted = std::fs::read(&first_path).unwrap();
        let second_vaults = second_dir.join("vaults");
        std::fs::create_dir_all(&second_vaults).unwrap();
        std::fs::write(second_vaults.join(format!("{}.vault", vault_id)), encrypted).unwrap();

        let mut first = VaultManager::new(first_dir).unwrap();
        let mut second = VaultManager::new(second_dir).unwrap();
        assert_ne!(first.device_id(), second.device_id());

        let first_vault = first.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();
        let second_vault = second.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();

        let first_clock = &first_vault.items[0].clock;
        let second_clock = &second_vault.items[0].clock;

        assert_eq!(first_clock.get(&first.device_id()), 1);
        assert_eq!(first_clock.get(&second.device_id()), 0);
        assert_eq!(second_clock.get(&second.device_id()), 1);
        assert_eq!(second_clock.get(&first.device_id()), 0);
        assert_eq!(
            first_clock.compare(second_clock),
            ClockOrder::Concurrent,
            "independently migrated copies must not look identical"
        );

        // Vault metadata is stamped the same way, so a later rename on one
        // Device is causally after the migration it saw.
        assert_eq!(
            first_vault.name_clock,
            FieldVersion::new(first.device_id(), 1)
        );
        assert_eq!(
            second_vault.name_clock,
            FieldVersion::new(second.device_id(), 1)
        );
        assert_ne!(first_vault.name_clock, second_vault.name_clock);
    }

    #[test]
    fn test_migration_overwrites_existing_backup() {
        let temp_dir = setup();
        let (accounts, vault_id, _) = legacy_accounts();
        write_legacy_vault_file(&temp_dir, vault_id, &accounts);

        let vault_path = temp_dir.join("vaults").join(format!("{}.vault", vault_id));
        let backup_path = temp_dir
            .join("vaults")
            .join(format!("{}.vault.OLD", vault_id));

        // A stale backup from an earlier migration must be overwritten
        std::fs::write(&backup_path, b"stale-backup").unwrap();
        let raw_before = std::fs::read(&vault_path).unwrap();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        manager.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();

        assert_eq!(
            std::fs::read(&backup_path).unwrap(),
            raw_before,
            "Backup must hold the pre-migration file, not a stale copy"
        );
    }

    #[test]
    fn test_corrupt_vault_fails_unlock_without_modification() {
        let temp_dir = setup();
        let vault_id = Uuid::new_v4();

        // Write an encrypted file whose plaintext is neither vault format
        let json = b"definitely not a vault schema";
        let salt = [7u8; 32];
        let master_key = derive_master_key(MASTER_PW, &KEY, &salt).unwrap();
        let encrypted = encrypt_with_key(&master_key, &salt, json).unwrap();
        let vaults_dir = temp_dir.join("vaults");
        std::fs::create_dir_all(&vaults_dir).unwrap();
        let vault_path = vaults_dir.join(format!("{}.vault", vault_id));
        std::fs::write(&vault_path, &encrypted).unwrap();
        let original = std::fs::read(&vault_path).unwrap();

        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let result = manager.unlock_vault(vault_id, MASTER_PW, &KEY);

        match result {
            Err(VaultError::Serialization(_)) => {}
            other => panic!("Expected serialization error, got: {:?}", other.map(|v| v.id)),
        }

        // Original file untouched, no backup created
        assert_eq!(std::fs::read(&vault_path).unwrap(), original);
        assert!(!temp_dir
            .join("vaults")
            .join(format!("{}.vault.OLD", vault_id))
            .exists());
    }

    #[test]
    fn test_save_refuses_when_vault_file_changed_externally() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Reopen the vault so the manager records the fingerprint of the
        // file it last read, as it would after a normal unlock.
        manager.lock_vault(Some(vault.id));
        manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();

        // Something else replaces the vault file behind the manager's back
        let vault_path = temp_dir.join("vaults").join(format!("{}.vault", vault.id));
        let foreign_contents = b"a different writer's vault file";
        std::fs::write(&vault_path, foreign_contents).unwrap();

        // A mutation tries to save over the foreign file
        let result = manager.update_vault(vault.id, Some("Renamed".to_string()), None);

        match result {
            Err(VaultError::ExternalChange(message)) => {
                assert!(
                    message.contains(&vault.id.to_string()),
                    "the error must identify the vault: {message}"
                );
            }
            other => panic!(
                "expected an external-change error, got: {:?}",
                other.map(|()| ())
            ),
        }

        // The other writer's file is left exactly as it was
        assert_eq!(
            std::fs::read(&vault_path).unwrap(),
            foreign_contents,
            "an externally changed vault must never be overwritten"
        );
    }

    #[test]
    fn test_create_and_unlock() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();

        let created = manager
            .create_vault(
                "My Vault".to_string(),
                "#6240BF".to_string(),
                MASTER_PW,
                &KEY,
            )
            .unwrap();
        assert_eq!(created.name, "My Vault");
        assert_eq!(created.color, "#6240BF");
        assert_eq!(created.schema_version, CURRENT_SCHEMA_VERSION);

        manager.lock_vault(Some(created.id));
        assert!(!manager.is_vault_unlocked(created.id));

        let unlocked = manager.unlock_vault(created.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(unlocked.id, created.id);
        assert!(manager.is_vault_unlocked(created.id));

        // A fresh vault must contain no items and should not need migration
        assert_eq!(unlocked.items.len(), 0);
        assert_eq!(unlocked.secrets.len(), 0);
    }

    #[test]
    fn test_add_account_item_through_item_surface() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // The account kind is created through the plain item command, its
        // initial password riding the shared SecretInput mechanism
        let item = manager
            .add_item(
                vault.id,
                Some("octocat pass".to_string()),
                None,
                crate::NewItemData::Account(crate::AccountData {
                    username: "octocat".to_string(),
                    email: None,
                }),
                Some(vec![crate::SecretInput::new(
                    SECRET_FIELD_PASSWORD,
                    "secret123".to_string(),
                )]),
            )
            .unwrap();
        let account = match &item {
            SafeItem::Account(v) => v,
            other => panic!("expected account safe view, got {other:?}"),
        };

        assert_eq!(account.display_name, Some("octocat pass".to_string()));
        assert_eq!(account.username, "octocat");
        assert_eq!(
            manager
                .get_item_secret(vault.id, account.id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("secret123")
        );
        // ...and unknown keys are rejected for the account kind too
        assert!(manager
            .get_item_secret(vault.id, account.id, SECRET_FIELD_CARD_NUMBER)
            .is_err());
    }

    #[test]
    fn test_update_account_item_through_item_surface() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let item = add_test_account_item(&mut manager, vault.id, "rspencil", "hunter2");
        let id = match &item {
            SafeItem::Account(v) => v.id,
            _ => unreachable!(),
        };

        let updated = match manager
            .update_item(
                vault.id,
                id,
                Some("Octo renamed".to_string()),
                None,
                Some(true),
                None,
                None,
                Some(crate::NewItemData::Account(crate::AccountData {
                    username: "newcat".to_string(),
                    email: Some("octo@example.com".to_string()),
                })),
            )
            .unwrap()
        {
            SafeItem::Account(a) => a,
            other => panic!("expected account safe view, got {other:?}"),
        };

        assert_eq!(updated.username, "newcat");
        assert_eq!(updated.email, Some("octo@example.com".to_string()));
        assert_eq!(updated.display_name, Some("Octo renamed".to_string()));
        assert_eq!(updated.favourite, true);

        // An emptied email persists as 'not set' (shared text-field rule)
        let cleared = match manager
            .update_item(
                vault.id,
                id,
                None,
                None,
                None,
                None,
                None,
                Some(crate::NewItemData::Account(crate::AccountData {
                    username: "newcat".to_string(),
                    email: Some(String::new()),
                })),
            )
            .unwrap()
        {
            SafeItem::Account(a) => a,
            other => panic!("expected account safe view, got {other:?}"),
        };
        assert_eq!(cleared.email, None);

        // The password did not change (it rides update_item_secret only)
        assert_eq!(
            manager
                .get_item_secret(vault.id, id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn test_item_password_strength_surface() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // With a stored password: strength derives
        let item = add_test_account_item(&mut manager, vault.id, "rspencil", "hunter2");
        let id = match &item {
            SafeItem::Account(v) => v.id,
            _ => unreachable!(),
        };
        assert!(manager.get_item_password_strength(vault.id, id).is_ok());

        // Passwordless account: a failed lookup, surfaced to the UI as
        // 'no strength' (the frontend wrapper catches it)
        let empty = manager
            .add_item(
                vault.id,
                None,
                None,
                crate::NewItemData::Account(crate::AccountData {
                    username: "no-pass".to_string(),
                    email: None,
                }),
                None,
            )
            .unwrap();
        let no_password_id = match &empty {
            SafeItem::Account(v) => v.id,
            _ => unreachable!(),
        };
        assert!(matches!(
            manager.get_item_password_strength(vault.id, no_password_id),
            Err(VaultError::Validation(ref msg)) if msg.contains("no password")
        ));
    }

    #[test]
    fn test_batch_mode_dirty_tracking() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Disable autosave for batch operations
        manager.set_autosave(false);

        // Add 3 accounts — none should be persisted yet
        for i in 0..3 {
            add_test_account_item(
                &mut manager,
                vault.id,
                &format!("user{}", i),
                &format!("pass{}", i),
            );
        }

        assert!(manager.is_dirty(vault.id));

        // Lock and re-unlock — data should NOT be there (wasn't flushed)
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(
            re_unlocked.items.len(),
            0,
            "Batch changes were not flushed"
        );

        // Re-add in batch mode
        manager.set_autosave(false);
        for i in 0..3 {
            add_test_account_item(
                &mut manager,
                vault.id,
                &format!("user{}", i),
                &format!("pass{}", i),
            );
        }
        assert!(manager.is_dirty(vault.id));

        // Flush explicitly
        manager.flush_vault(vault.id).unwrap();
        assert!(!manager.is_dirty(vault.id));

        // Re-enable autosave
        manager.set_autosave(true);

        // Lock and re-unlock — data SHOULD be there now
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(
            re_unlocked.items.len(),
            3,
            "Flushed changes were not persisted"
        );
    }

    #[test]
    fn test_flush_all() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();

        let vault1 = manager
            .create_vault("V1".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault2 = manager
            .create_vault("V2".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        manager.set_autosave(false);

        add_test_account_item(&mut manager, vault1.id, "u1", "p1");
        add_test_account_item(&mut manager, vault2.id, "u2", "p2");

        assert!(manager.is_dirty(vault1.id));
        assert!(manager.is_dirty(vault2.id));

        manager.flush_all().unwrap();

        assert!(!manager.is_dirty(vault1.id));
        assert!(!manager.is_dirty(vault2.id));

        // Verify persistence
        manager.lock_vault(None);
        let v1 = manager.unlock_vault(vault1.id, MASTER_PW, &KEY).unwrap();
        let v2 = manager.unlock_vault(vault2.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(v1.items.len(), 1);
        assert_eq!(v2.items.len(), 1);
    }

    #[test]
    fn test_persistence_after_autosave() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Autosave is on by default — add account and verify it persists
        add_test_account_item(&mut manager, vault.id, "octocat", "pass");

        assert!(
            !manager.is_dirty(vault.id),
            "Autosave should have cleared dirty flag"
        );

        // Lock and re-unlock to verify persistence
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(re_unlocked.items.len(), 1);
    }

    #[test]
    fn test_new_items_carry_local_device_clock() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let item = add_test_account_item(&mut manager, vault.id, "octocat", "hunter2");
        let item_id = match item {
            SafeItem::Account(view) => view.id,
            _ => panic!("expected account view"),
        };

        let stored = manager
            .get_vault(vault.id)
            .unwrap()
            .items
            .iter()
            .find(|stored| stored.id == item_id)
            .unwrap();

        assert_eq!(stored.clock.get(&device), 1);
        assert_eq!(stored.clock.iter().count(), 1);
        assert!(!stored.is_content_free());

        for key in [
            "display_name",
            "favourite",
            "tags",
            "icon",
            "color",
            "username",
            "email",
            "password",
        ] {
            assert_eq!(
                stored.field_clocks.get(key),
                Some(&FieldVersion::new(device, 1)),
                "field '{key}' must carry this Device's creation version"
            );
        }

        let secret_entry = manager
            .get_vault(vault.id)
            .unwrap()
            .secrets
            .iter()
            .find(|entry| entry.item_id == item_id)
            .unwrap();
        assert_eq!(
            secret_entry.fields[0].clock,
            FieldVersion::new(device, 1)
        );
    }

    #[test]
    fn test_item_update_ticks_only_touched_field_clocks() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let item = add_test_account_item(&mut manager, vault.id, "octocat", "hunter2");
        let item_id = match item {
            SafeItem::Account(view) => view.id,
            _ => panic!("expected account view"),
        };

        // Renaming touches display_name only, but ticks the lifecycle clock.
        manager
            .update_item(
                vault.id,
                item_id,
                Some("Renamed".to_string()),
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let stored = manager
            .get_vault(vault.id)
            .unwrap()
            .items
            .iter()
            .find(|stored| stored.id == item_id)
            .unwrap();
        assert_eq!(stored.clock.get(&device), 2);
        assert_eq!(
            stored.field_clocks.get("display_name"),
            Some(&FieldVersion::new(device, 2))
        );
        for untouched in ["username", "email", "favourite", "tags"] {
            assert_eq!(
                stored.field_clocks.get(untouched),
                Some(&FieldVersion::new(device, 1)),
                "untouched field '{untouched}' must keep its earlier version"
            );
        }

        // Replacing kind data stamps the fields that actually changed.
        manager
            .update_item(
                vault.id,
                item_id,
                None,
                None,
                None,
                None,
                None,
                Some(crate::NewItemData::Account(crate::AccountData {
                    username: "newcat".to_string(),
                    email: Some("new@example.com".to_string()),
                })),
            )
            .unwrap();

        let stored = manager
            .get_vault(vault.id)
            .unwrap()
            .items
            .iter()
            .find(|stored| stored.id == item_id)
            .unwrap();
        assert_eq!(stored.clock.get(&device), 3);
        for changed in ["username", "email"] {
            assert_eq!(
                stored.field_clocks.get(changed),
                Some(&FieldVersion::new(device, 3))
            );
        }
        assert_eq!(
            stored.field_clocks.get("display_name"),
            Some(&FieldVersion::new(device, 2))
        );
    }

    #[test]
    fn test_secret_update_ticks_item_and_secret_versions() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let item = add_test_account_item(&mut manager, vault.id, "octocat", "hunter2");
        let item_id = match item {
            SafeItem::Account(view) => view.id,
            _ => panic!("expected account view"),
        };

        manager
            .update_item_secret(
                vault.id,
                item_id,
                SECRET_FIELD_PASSWORD,
                Some("new_password".to_string()),
            )
            .unwrap();

        {
            let vault_ref = manager.get_vault(vault.id).unwrap();
            let stored = vault_ref
                .items
                .iter()
                .find(|stored| stored.id == item_id)
                .unwrap();
            assert_eq!(stored.clock.get(&device), 2);
            assert_eq!(
                stored.field_clocks.get(SECRET_FIELD_PASSWORD),
                Some(&FieldVersion::new(device, 2))
            );
            let secret = vault_ref
                .secrets
                .iter()
                .find(|entry| entry.item_id == item_id)
                .unwrap();
            assert_eq!(
                secret.fields[0].clock,
                FieldVersion::new(device, 2)
            );
        }

        // Clearing is a write too: the field keeps the newer clock even
        // though no secret value remains.
        manager
            .update_item_secret(vault.id, item_id, SECRET_FIELD_PASSWORD, None)
            .unwrap();

        let vault_ref = manager.get_vault(vault.id).unwrap();
        let stored = vault_ref
            .items
            .iter()
            .find(|stored| stored.id == item_id)
            .unwrap();
        assert_eq!(stored.clock.get(&device), 3);
        assert_eq!(
            stored.field_clocks.get(SECRET_FIELD_PASSWORD),
            Some(&FieldVersion::new(device, 3))
        );
        assert!(vault_ref.secrets.iter().all(|s| s.item_id != item_id));
    }

    // ============ Item command surface (notes first) ============

    fn add_test_note(manager: &mut VaultManager, vault_id: VaultId, title: &str, body: &str) -> SafeItem {
        manager
            .add_item(
                vault_id,
                Some("Notebook".to_string()),
                None,
                crate::NewItemData::Note(crate::NoteData {
                    title: Some(title.to_string()),
                    body: body.to_string(),
                }),
                None,
            )
            .unwrap()
    }

    #[test]
    fn test_add_note_item() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let note = add_test_note(&mut manager, vault.id, "Recipe", "Pasta carbonara for two");

        // Safe view is correctly tagged and carries the kind fields
        let json = serde_json::to_value(&note).unwrap();
        assert_eq!(json["kind"], "note");
        assert_eq!(json["title"], "Recipe");
        assert_eq!(json["body"], "Pasta carbonara for two");
        assert_eq!(json["display_name"], "Notebook");
        assert!(json.get("password").is_none(), "Items carry no secrets");

        // Parses back into the SafeItem union as the note variant
        // (checked structurally — safe views are never round-tripped)
        assert_eq!(json["title"], "Recipe");
        assert_eq!(json["body"], "Pasta carbonara for two");

        // Stored in the vault
        let vault_ref = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault_ref.items.len(), 1);
        assert!(
            vault_ref.secrets.is_empty(),
            "Notes must have no secret entry"
        );
    }

    #[test]
    fn test_get_all_items_returns_mixed_kinds() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // The item surface carries the account kind: search/list over all kinds
        add_test_account_item(&mut manager, vault.id, "octocat", "pw");
        add_test_note(&mut manager, vault.id, "Recipe", "Pasta carbonara for two");

        let items = manager
            .get_all_items(crate::ItemFilter::default())
            .unwrap();
        assert_eq!(items.len(), 2);
        let kinds: Vec<&str> = items
            .iter()
            .map(|i| match i {
                crate::SafeItem::Account(_) => "account",
                crate::SafeItem::Note(_) => "note",
                crate::SafeItem::Card(_) => "card",
                crate::SafeItem::Identity(_) => "identity",
            })
            .collect();
        assert!(kinds.contains(&"account"));
        assert!(kinds.contains(&"note"));

        // Search reaches the account's own kind fields
        let by_username = manager
            .get_all_items(crate::ItemFilter {
                search_query: Some("octocat".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_username.len(), 1);
        assert!(matches!(
            &by_username[0],
            crate::SafeItem::Account(a) if a.username == "octocat"
        ));
    }

    #[test]
    fn test_update_note_item_persists() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let note = add_test_note(&mut manager, vault.id, "Recipe", "Old body");

        let updated = manager
            .update_item(
                vault.id,
                match &note {
                    crate::SafeItem::Note(n) => n.id,
                    _ => panic!("expected note"),
                },
                Some("Cookbook".to_string()),
                None,
                Some(true),
                Some(vec!["kitchen".to_string()]),
                None,
                Some(crate::NewItemData::Note(crate::NoteData {
                    title: Some("Recipe v2".to_string()),
                    body: "New body".to_string(),
                })),
            )
            .unwrap();

        let crate::SafeItem::Note(view) = &updated else {
            panic!("expected note view");
        };
        assert!(view.favourite);
        assert_eq!(view.display_name, Some("Cookbook".to_string()));
        assert_eq!(view.title.as_deref(), Some("Recipe v2"));
        assert_eq!(view.body, "New body");
        assert_eq!(view.tags, vec!["kitchen".to_string()]);

        // Persists across unlock
        manager.lock_vault(Some(vault.id));
        manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        let items = manager
            .get_all_items(crate::ItemFilter::default())
            .unwrap();
        let crate::SafeItem::Note(view) = &items[0] else {
            panic!("expected note view");
        };
        assert!(view.favourite);
        assert_eq!(view.title.as_deref(), Some("Recipe v2"));
        assert_eq!(view.body, "New body");
    }

    #[test]
    fn test_get_item_by_id_returns_tagged_safe_item() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let account = add_test_account_item(&mut manager, vault.id, "octocat", "pw");
        let account_id = match &account {
            crate::SafeItem::Account(a) => a.id,
            _ => unreachable!(),
        };
        let note = add_test_note(&mut manager, vault.id, "Recipe", "Body");

        let by_id = manager.get_item_by_id(account_id).unwrap();
        assert!(matches!(by_id, crate::SafeItem::Account(_)));

        let note_id = match &note {
            crate::SafeItem::Note(n) => n.id,
            _ => panic!("expected note"),
        };
        let by_id = manager.get_item_by_id(note_id).unwrap();
        assert!(matches!(by_id, crate::SafeItem::Note(_)));

        let missing = manager.get_item_by_id(Uuid::new_v4());
        assert!(matches!(missing, Err(VaultError::ItemNotFound(_))));
    }

    #[test]
    fn test_delete_item_retains_tombstone_without_payload_or_secrets() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let note = add_test_note(&mut manager, vault.id, "Note", "Body");
        let note_id = match &note {
            crate::SafeItem::Note(n) => n.id,
            _ => panic!("expected note"),
        };
        manager.delete_item(vault.id, note_id).unwrap();

        {
            let vault_ref = manager.get_vault(vault.id).unwrap();
            assert_eq!(vault_ref.items.len(), 1, "the tombstone is retained");
            let tombstone = &vault_ref.items[0];
            assert!(tombstone.is_deleted());
            assert!(tombstone.kind.is_none(), "a tombstone carries no payload");
            assert_eq!(tombstone.id, note_id);
            assert_eq!(tombstone.clock.get(&device), 2);
            assert!(tombstone.field_clocks.is_empty());
            assert!(tombstone.display_name.is_none());
            assert!(tombstone.tags.is_empty());
        }

        // Tombstones are retained for sync but never listed or fetched.
        assert!(manager
            .get_all_items(ItemFilter::default())
            .unwrap()
            .is_empty());
        assert!(matches!(
            manager.get_item_by_id(note_id),
            Err(VaultError::ItemNotFound(_))
        ));
        assert!(matches!(
            manager.get_item_secret(vault.id, note_id, SECRET_FIELD_PASSWORD),
            Err(VaultError::ItemNotFound(_))
        ));

        // delete_item is kind-generic and drops secrets too.
        let account = add_test_account_item(&mut manager, vault.id, "octocat", "pw");
        let account_id = match &account {
            SafeItem::Account(a) => a.id,
            _ => unreachable!(),
        };
        manager.delete_item(vault.id, account_id).unwrap();
        {
            let vault_ref = manager.get_vault(vault.id).unwrap();
            assert_eq!(vault_ref.items.len(), 2);
            assert!(vault_ref.items.iter().all(|item| item.is_deleted()));
            assert!(vault_ref.secrets.is_empty(), "Deleting must drop secrets too");
            let account_tombstone = vault_ref
                .items
                .iter()
                .find(|item| item.id == account_id)
                .unwrap();
            assert_eq!(account_tombstone.clock.get(&device), 2);
        }

        // A saved v3 vault reloads its tombstones intact.
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(re_unlocked.items.len(), 2);
        assert!(re_unlocked.items.iter().all(|item| item.is_content_free()));
        assert!(manager
            .get_all_items(ItemFilter::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_deleting_an_already_deleted_item_is_idempotent() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let note = add_test_note(&mut manager, vault.id, "Note", "Body");
        let note_id = match &note {
            crate::SafeItem::Note(n) => n.id,
            _ => panic!("expected note"),
        };

        manager.delete_item(vault.id, note_id).unwrap();
        let clock_after_first = {
            let vault_ref = manager.get_vault(vault.id).unwrap();
            let tombstone = vault_ref
                .items
                .iter()
                .find(|item| item.id == note_id)
                .unwrap();
            tombstone.clock.clone()
        };

        // The same delete arriving twice (a retry, or two Devices deleting
        // the same Item) must succeed quietly.
        manager.delete_item(vault.id, note_id).unwrap();

        let vault_ref = manager.get_vault(vault.id).unwrap();
        let tombstones: Vec<_> = vault_ref.items.iter().filter(|i| i.deleted).collect();
        assert_eq!(tombstones.len(), 1, "one identity has one Tombstone");
        assert_eq!(
            tombstones[0].clock, clock_after_first,
            "a repeat delete must not advance the clock"
        );
        assert_eq!(tombstones[0].clock.get(&device), 2);

        // A move record is not a deletion: an Item that moved away is not
        // "already deleted" in the vault it left.
        let destination = manager
            .create_vault("Elsewhere".to_string(), "#112233".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let moved = add_test_account_item(&mut manager, vault.id, "octocat", "pw");
        let moved_id = match &moved {
            crate::SafeItem::Account(account) => account.id,
            _ => panic!("expected account"),
        };
        manager
            .move_item(vault.id, moved_id, destination.id)
            .unwrap();

        assert!(matches!(
            manager.delete_item(vault.id, moved_id),
            Err(VaultError::ItemNotFound(_))
        ));

        // An identity this vault never held is still an error.
        assert!(matches!(
            manager.delete_item(vault.id, Uuid::new_v4()),
            Err(VaultError::ItemNotFound(_))
        ));
    }

    #[test]
    fn test_removing_a_local_copy_deletes_only_this_devices_file() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        manager.remove_local_copy(vault.id).unwrap();

        assert!(!manager.has_vault_file(vault.id), "the local file is gone");
        assert!(matches!(
            manager.get_vault(vault.id),
            Err(VaultError::VaultLocked)
        ));
        assert!(
            !manager.vault_trash_path(vault.id).exists(),
            "removing a local copy is not a deletion and leaves no trash"
        );
    }

    #[test]
    fn test_moving_a_deleted_vault_to_local_trash_keeps_the_encrypted_file() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        manager.lock_vault(Some(vault.id));
        let encrypted =
            std::fs::read(temp_dir.join("vaults").join(format!("{}.vault", vault.id))).unwrap();

        assert!(manager.move_to_trash(vault.id).unwrap());

        assert!(!manager.has_vault_file(vault.id));
        let trash = manager.vault_trash_path(vault.id);
        assert_eq!(
            std::fs::read(&trash).unwrap(),
            encrypted,
            "trash keeps the encrypted file rather than destroying it"
        );

        // A deletion can arrive twice; the second one is a no-op.
        assert!(!manager.move_to_trash(vault.id).unwrap());
        assert!(trash.exists());
    }

    #[test]
    fn test_a_failed_install_does_not_publish_an_unlocked_vault() {
        let source_dir = setup();
        let mut source = VaultManager::new(source_dir.clone()).unwrap();
        let vault = source
            .create_vault("Remote".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let target_dir = setup();
        let mut target = VaultManager::new(target_dir.clone()).unwrap();
        // Make the destination unwritable by putting a directory where the
        // vault file must go.
        let blocked = target_dir.join("vaults").join(format!("{}.vault", vault.id));
        std::fs::create_dir(&blocked).unwrap();

        let result = target.install_vault(vault.clone(), MASTER_PW, &KEY);

        assert!(result.is_err());
        assert!(
            matches!(target.get_vault(vault.id), Err(VaultError::VaultLocked)),
            "a vault whose file was never written must not look unlocked"
        );

        std::fs::remove_dir_all(source_dir).ok();
        std::fs::remove_dir_all(target_dir).ok();
    }

    #[test]
    fn test_card_expiry_month_is_validated_on_update() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let card = add_test_card(&mut manager, vault.id);
        let card_id = match card {
            crate::SafeItem::Card(view) => view.id,
            _ => panic!("expected card view"),
        };
        let clock_before = manager
            .get_vault(vault.id)
            .unwrap()
            .items
            .iter()
            .find(|item| item.id == card_id)
            .unwrap()
            .clock
            .clone();

        let result = manager.update_item(
            vault.id,
            card_id,
            None,
            None,
            None,
            None,
            None,
            Some(crate::NewItemData::Card(crate::CardData {
                cardholder_name: "Alex".to_string(),
                expiry_month: 13,
                expiry_year: 2030,
                brand: None,
            })),
        );

        assert!(matches!(
            result,
            Err(VaultError::Validation(ref msg)) if msg.contains("between 1 and 12")
        ));
        let stored = manager
            .get_vault(vault.id)
            .unwrap()
            .items
            .iter()
            .find(|item| item.id == card_id)
            .unwrap();
        assert_eq!(stored.clock, clock_before, "a rejected update is not a write");
        assert_eq!(stored.clock.get(&device), clock_before.get(&device));
    }

    #[test]
    fn test_move_item_between_vaults() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault_a = manager
            .create_vault("A".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault_b = manager
            .create_vault("B".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let note = add_test_note(&mut manager, vault_a.id, "Recipe", "Body");
        let note_id = match &note {
            crate::SafeItem::Note(n) => n.id,
            _ => panic!("expected note"),
        };
        // Make the note searchable metadata we expect to survive the move
        manager
            .update_item(
                vault_a.id,
                note_id,
                None,
                None,
                Some(true),
                Some(vec!["kitchen".to_string()]),
                None,
                None,
            )
            .unwrap();

        let account = add_test_account_item(&mut manager, vault_a.id, "octocat", "hunter2");
        let account_id = match &account {
            crate::SafeItem::Account(a) => a.id,
            _ => panic!("expected account view"),
        };

        // Same-vault and missing-item moves are rejected
        let same = manager.move_item(vault_a.id, note_id, vault_a.id);
        assert!(matches!(same, Err(VaultError::Validation(_))));
        let missing = manager.move_item(vault_a.id, Uuid::new_v4(), vault_b.id);
        assert!(matches!(missing, Err(VaultError::ItemNotFound(_))));

        // Move the note
        let moved_note = manager.move_item(vault_a.id, note_id, vault_b.id).unwrap();
        let crate::SafeItem::Note(note_view) = &moved_note else {
            panic!("expected note view");
        };
        assert_eq!(note_view.id, note_id, "a move keeps the item's identity");
        assert_eq!(note_view.vault_id, vault_b.id);
        assert!(note_view.favourite, "metadata must survive the move");
        assert_eq!(note_view.tags, vec!["kitchen".to_string()]);
        // It is still findable - now in the target vault, not the source.
        assert!(matches!(
            manager.get_item_by_id(note_id).unwrap(),
            SafeItem::Note(view) if view.vault_id == vault_b.id
        ));
        assert!(manager
            .get_vault(vault_a.id)
            .unwrap()
            .items
            .iter()
            .find(|i| i.id == note_id)
            .is_some_and(|record| record.is_move_record() && record.moved_to == Some(vault_b.id)),
            "the source vault keeps only a move record for the item");

        // Move the account: its secret must be remapped, not dropped
        let moved_account = manager.move_item(vault_a.id, account_id, vault_b.id).unwrap();
        let account_view = match &moved_account {
            crate::SafeItem::Account(view) => view.clone(),
            _ => panic!("expected account view"),
        };
        assert_eq!(account_view.vault_id, vault_b.id);

        let secret = manager
            .get_item_secret(vault_b.id, account_view.id, SECRET_FIELD_PASSWORD)
            .unwrap();
        assert_eq!(secret.as_deref(), Some("hunter2"), "secret entry must move with the item");

        let vault_ref = manager.get_vault(vault_a.id).unwrap();
        assert!(
            vault_ref.items.iter().all(|item| item.is_move_record()),
            "the source vault keeps move records, not live items"
        );
        assert_eq!(vault_ref.items.len(), 2, "one record per moved item");
        assert!(vault_ref
            .items
            .iter()
            .all(|item| item.moved_to == Some(vault_b.id)));
        assert!(vault_ref.secrets.is_empty());
    }

    // ============ Identity items (second pipeline kind) ============

    fn add_test_identity(manager: &mut VaultManager, vault_id: VaultId) -> SafeItem {
        manager
            .add_item(
                vault_id,
                Some("Me".to_string()),
                None,
                crate::NewItemData::Identity(crate::IdentityData {
                    full_name: "Alex Doe".to_string(),
                    email: Some("alex@example.com".to_string()),
                    phone: Some("+1 555 0100".to_string()),
                    address_line1: Some("1 Main St".to_string()),
                    address_line2: None,
                    city: Some("Springfield".to_string()),
                    postal_code: Some("12345".to_string()),
                    country: Some("US".to_string()),
                }),
                None,
            )
            .unwrap()
    }

    #[test]
    fn test_add_identity_item() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let identity = add_test_identity(&mut manager, vault.id);

        let json = serde_json::to_value(&identity).unwrap();
        assert_eq!(json["kind"], "identity");
        assert_eq!(json["full_name"], "Alex Doe");
        assert_eq!(json["city"], "Springfield");
        assert!(json.get("password").is_none(), "Items carry no secrets");

        // Stored without a secret entry
        let vault_ref = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault_ref.items.len(), 1);
        assert!(vault_ref.secrets.is_empty());

        // Tagged correctly when fetched by id
        let by_id = match manager.get_item_by_id(
            match &identity {
                crate::SafeItem::Identity(i) => i.id,
                _ => panic!("expected identity"),
            },
        ) {
            Ok(crate::SafeItem::Identity(_)) => true,
            _ => false,
        };
        assert!(by_id);
    }

    #[test]
    fn test_update_identity_item_persists() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let identity = add_test_identity(&mut manager, vault.id);
        let identity_id = match &identity {
            crate::SafeItem::Identity(i) => i.id,
            _ => panic!("expected identity"),
        };

        manager
            .update_item(
                vault.id,
                identity_id,
                Some("Emergency contact".to_string()),
                None,
                Some(true),
                Some(vec!["family".to_string()]),
                None,
                Some(crate::NewItemData::Identity(crate::IdentityData {
                    full_name: "Alex Doe Jr".to_string(),
                    email: None,
                    phone: Some("+1 555 0199".to_string()),
                    address_line1: Some("2 Side St".to_string()),
                    address_line2: Some("Apt 3".to_string()),
                    city: None,
                    postal_code: None,
                    country: Some("US".to_string()),
                })),
            )
            .unwrap();

        manager.lock_vault(Some(vault.id));
        manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();

        let items = manager
            .get_all_items(crate::ItemFilter::default())
            .unwrap();
        let crate::SafeItem::Identity(view) = &items[0] else {
            panic!("expected identity view");
        };
        assert!(view.favourite);
        assert_eq!(view.display_name.as_deref(), Some("Emergency contact"));
        assert_eq!(view.full_name, "Alex Doe Jr");
        assert_eq!(view.email, None);
        assert_eq!(view.phone.as_deref(), Some("+1 555 0199"));
        assert_eq!(view.address_line1.as_deref(), Some("2 Side St"));
        assert_eq!(view.address_line2.as_deref(), Some("Apt 3"));
        assert_eq!(view.city, None);
        assert_eq!(view.tags, vec!["family".to_string()]);
    }

    #[test]
    fn test_identity_search_matches_fields() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        add_test_identity(&mut manager, vault.id);

        for query in ["alex doe", "example.com", "555 0100", "1 main st", "US"] {
            let matches = manager
                .get_all_items(crate::ItemFilter {
                    vault_id: Some(vault.id),
                    search_query: Some(query.to_string()),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(matches.len(), 1, "query '{query}' must match identity");
        }

        let misses = manager
            .get_all_items(crate::ItemFilter {
                vault_id: Some(vault.id),
                search_query: Some("unrelated".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert!(misses.is_empty());
    }

    #[test]
    fn test_identity_move_and_delete() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault_a = manager
            .create_vault("A".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault_b = manager
            .create_vault("B".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let identity = add_test_identity(&mut manager, vault_a.id);
        let identity_id = match &identity {
            crate::SafeItem::Identity(i) => i.id,
            _ => panic!("expected identity"),
        };

        let moved = manager.move_item(vault_a.id, identity_id, vault_b.id).unwrap();
        assert!(matches!(moved, crate::SafeItem::Identity(_)));
        assert_eq!(
            manager.get_vault(vault_a.id).unwrap().items.len(),
            1,
            "the source vault keeps a move record"
        );
        assert!(manager.get_vault(vault_a.id).unwrap().items[0].is_move_record());

        let new_id = match &moved {
            crate::SafeItem::Identity(i) => i.id,
            _ => unreachable!(),
        };
        manager.delete_item(vault_b.id, new_id).unwrap();
        let vault_b_ref = manager.get_vault(vault_b.id).unwrap();
        assert_eq!(vault_b_ref.items.len(), 1, "deletion leaves a Tombstone");
        assert!(vault_b_ref.items[0].is_deleted());
        assert_eq!(vault_b_ref.items[0].id, new_id);
    }

    #[test]
    fn test_item_search_matches_note_title_and_body() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // An account with an email, so the account-arm search is covered
        let _account = manager
            .add_item(
                vault.id,
                Some("Old trusted".to_string()),
                None,
                crate::NewItemData::Account(crate::AccountData {
                    username: "octocat".to_string(),
                    email: Some("octo@example.com".to_string()),
                }),
                Some(vec![crate::SecretInput::new(
                    SECRET_FIELD_PASSWORD,
                    "pw".to_string(),
                )]),
            )
            .unwrap();
        add_test_note(&mut manager, vault.id, "Recipe", "Pasta carbonara for two");

        let matches_body = manager
            .get_all_items(crate::ItemFilter {
                vault_id: Some(vault.id),
                search_query: Some("CARBONARA".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(matches_body.len(), 1, "body text must be searchable");

        let matches_title = manager
            .get_all_items(crate::ItemFilter {
                vault_id: Some(vault.id),
                search_query: Some("recipe".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(matches_title.len(), 1, "title must be searchable");

        let matches_account = manager
            .get_all_items(crate::ItemFilter {
                vault_id: Some(vault.id),
                search_query: Some("octo@example.com".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(matches_account.len(), 1);
        assert!(matches!(matches_account[0], crate::SafeItem::Account(_)));
    }

    // ============ Card items (per-field secrets) ============

    fn add_test_card(manager: &mut VaultManager, vault_id: VaultId) -> SafeItem {
        manager
            .add_item(
                vault_id,
                Some("Travel card".to_string()),
                None,
                crate::NewItemData::Card(crate::CardData {
                    cardholder_name: "Alex Doe".to_string(),
                    expiry_month: 11,
                    expiry_year: 2030,
                    brand: Some("Visa".to_string()),
                }),
                Some(vec![
                    crate::SecretInput::new(
                        crate::SECRET_FIELD_CARD_NUMBER,
                        "4111 1111 1111 1234".to_string(),
                    ),
                    crate::SecretInput::new(
                        crate::SECRET_FIELD_SECURITY_CODE,
                        "123".to_string(),
                    ),
                    crate::SecretInput::new(crate::SECRET_FIELD_PIN, "4321".to_string()),
                ]),
            )
            .unwrap()
    }

    fn add_test_account_item(
        manager: &mut VaultManager,
        vault_id: VaultId,
        username: &str,
        password: &str,
    ) -> SafeItem {
        manager
            .add_item(
                vault_id,
                None,
                None,
                crate::NewItemData::Account(crate::AccountData {
                    username: username.to_string(),
                    email: None,
                }),
                Some(vec![crate::SecretInput::new(
                    SECRET_FIELD_PASSWORD,
                    password.to_string(),
                )]),
            )
            .unwrap()
    }

    fn add_named_account(
        manager: &mut VaultManager,
        vault_id: VaultId,
        display_name: &str,
        username: &str,
        password: &str,
    ) -> SafeItem {
        manager
            .add_item(
                vault_id,
                Some(display_name.to_string()),
                None,
                crate::NewItemData::Account(crate::AccountData {
                    username: username.to_string(),
                    email: None,
                }),
                Some(vec![crate::SecretInput::new(
                    SECRET_FIELD_PASSWORD,
                    password.to_string(),
                )]),
            )
            .unwrap()
    }

    fn safe_item_id(item: &SafeItem) -> ItemId {
        match item {
            SafeItem::Account(view) => view.id,
            SafeItem::Card(view) => view.id,
            SafeItem::Note(view) => view.id,
            SafeItem::Identity(view) => view.id,
        }
    }

    fn canonical_pair(first: ItemId, second: ItemId) -> (ItemId, ItemId) {
        if first <= second {
            (first, second)
        } else {
            (second, first)
        }
    }

    #[test]
    fn test_duplicate_suggestions_require_same_kind_normalised_name_and_account_username() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let first = add_named_account(&mut manager, vault.id, "GitHub", "octocat", "first-pw");
        let second = add_named_account(&mut manager, vault.id, "  gItHuB  ", "octocat", "second-pw");

        // A different username (including only a case difference), a
        // different kind, and an unnamed item are not Duplicates even when
        // the display name matches.
        add_named_account(&mut manager, vault.id, "GitHub", "someone-else", "third-pw");
        add_named_account(&mut manager, vault.id, "GitHub", "OctoCat", "case-pw");
        manager
            .add_item(
                vault.id,
                Some("GitHub".to_string()),
                None,
                crate::NewItemData::Note(crate::NoteData {
                    title: None,
                    body: "not an account".to_string(),
                }),
                None,
            )
            .unwrap();
        add_named_account(&mut manager, vault.id, "", "octocat", "fourth-pw");

        let before = serde_json::to_value(manager.get_vault(vault.id).unwrap()).unwrap();
        let suggestions = manager.duplicate_suggestions(vault.id).unwrap();
        let after = serde_json::to_value(manager.get_vault(vault.id).unwrap()).unwrap();

        assert_eq!(
            before, after,
            "listing suggestions must never change the vault"
        );
        assert_eq!(suggestions.len(), 1, "got {suggestions:?}");

        let pair = canonical_pair(safe_item_id(&first), safe_item_id(&second));
        assert_eq!(
            (suggestions[0].first.item_id, suggestions[0].second.item_id),
            pair
        );
        assert_eq!(suggestions[0].username.as_deref(), Some("octocat"));

        // The differing password needs an explicit Combine choice, but the
        // suggestion must never carry either secret value.
        let password = suggestions[0]
            .colliding_fields
            .iter()
            .find(|field| field.field_key == SECRET_FIELD_PASSWORD)
            .expect("a differing password collides");
        assert!(password.is_secret);
        assert!(password.first_value.is_none());
        assert!(password.second_value.is_none());
        let payload = serde_json::to_string(&suggestions).unwrap();
        assert!(!payload.contains("first-pw"));
        assert!(!payload.contains("second-pw"));
    }

    #[test]
    fn test_keep_both_dismisses_the_pair_and_persists_it_in_the_vault() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let first = add_named_account(&mut manager, vault.id, "GitHub", "octocat", "one");
        let second = add_named_account(&mut manager, vault.id, "GitHub", "octocat", "two");
        let (first_id, second_id) = (safe_item_id(&first), safe_item_id(&second));
        assert_eq!(manager.duplicate_suggestions(vault.id).unwrap().len(), 1);

        let outcome = manager
            .resolve_duplicate(
                vault.id,
                first_id,
                second_id,
                crate::DuplicateResolution::KeepBoth,
            )
            .unwrap();
        assert_eq!(outcome, crate::DuplicateOutcome::KeptBoth);

        assert!(manager.duplicate_suggestions(vault.id).unwrap().is_empty());
        assert!(manager.get_vault(vault.id).unwrap().is_duplicate_dismissed(first_id, second_id));

        // Both identities survive untouched.
        let vault_data = manager.get_vault(vault.id).unwrap();
        for id in [first_id, second_id] {
            assert!(vault_data
                .items
                .iter()
                .any(|item| item.id == id && !item.is_content_free()));
        }

        // The dismissal is vault content, so a lock/unlock round trip keeps it.
        manager.lock_vault(Some(vault.id));
        manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert!(manager.duplicate_suggestions(vault.id).unwrap().is_empty());
    }

    #[test]
    fn test_keep_one_identity_tombstones_the_other_and_keeps_its_secret() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let keeper = add_named_account(&mut manager, vault.id, "GitHub", "octocat", "keeper-pw");
        let discarded = add_named_account(&mut manager, vault.id, "GitHub", "octocat", "discarded-pw");
        let (keeper_id, discarded_id) = (safe_item_id(&keeper), safe_item_id(&discarded));

        // A survivor id that belongs to neither Item is rejected before any
        // mutation.
        let invalid = manager.resolve_duplicate(
            vault.id,
            keeper_id,
            discarded_id,
            crate::DuplicateResolution::Keep {
                item_id: Uuid::new_v4(),
            },
        );
        assert!(matches!(invalid, Err(VaultError::Validation(_))));
        assert_eq!(manager.duplicate_suggestions(vault.id).unwrap().len(), 1);

        let outcome = manager
            .resolve_duplicate(
                vault.id,
                keeper_id,
                discarded_id,
                crate::DuplicateResolution::Keep { item_id: keeper_id },
            )
            .unwrap();
        assert_eq!(outcome, crate::DuplicateOutcome::Kept { item_id: keeper_id });

        let vault_data = manager.get_vault(vault.id).unwrap();
        let keeper_item = vault_data
            .items
            .iter()
            .find(|item| item.id == keeper_id)
            .unwrap();
        assert!(!keeper_item.is_content_free());
        assert_eq!(
            manager
                .get_item_secret(vault.id, keeper_id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("keeper-pw")
        );

        let tombstone = vault_data
            .items
            .iter()
            .find(|item| item.id == discarded_id)
            .unwrap();
        assert!(tombstone.is_deleted());
        assert!(vault_data
            .secrets
            .iter()
            .all(|entry| entry.item_id != discarded_id));
        assert!(manager.duplicate_suggestions(vault.id).unwrap().is_empty());
    }

    fn add_duplicate_account(
        manager: &mut VaultManager,
        vault_id: VaultId,
        display_name: &str,
        username: &str,
        email: &str,
        password: &str,
        tags: &[&str],
        favourite: bool,
    ) -> SafeItem {
        let item = manager
            .add_item(
                vault_id,
                Some(display_name.to_string()),
                None,
                crate::NewItemData::Account(crate::AccountData {
                    username: username.to_string(),
                    email: Some(email.to_string()),
                }),
                Some(vec![crate::SecretInput::new(
                    SECRET_FIELD_PASSWORD,
                    password.to_string(),
                )]),
            )
            .unwrap();

        manager
            .update_item(
                vault_id,
                safe_item_id(&item),
                None,
                None,
                Some(favourite),
                Some(tags.iter().map(|tag| tag.to_string()).collect()),
                None,
                None,
            )
            .unwrap()
    }

    #[test]
    fn test_combine_refuses_to_pick_a_secret_without_an_explicit_choice() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let first = add_duplicate_account(
            &mut manager,
            vault.id,
            "GitHub",
            "octocat",
            "one@example.com",
            "first-pw",
            &[],
            false,
        );
        let second = add_duplicate_account(
            &mut manager,
            vault.id,
            "github",
            "octocat",
            "two@example.com",
            "second-pw",
            &[],
            false,
        );
        let (first_id, second_id) = (safe_item_id(&first), safe_item_id(&second));
        let before = serde_json::to_value(manager.get_vault(vault.id).unwrap()).unwrap();

        // No choices at all: every colliding field, the password included,
        // is left for the User to decide.
        let unresolved = manager.resolve_duplicate(
            vault.id,
            first_id,
            second_id,
            crate::DuplicateResolution::Combine {
                survivor: first_id,
                field_choices: Default::default(),
            },
        );
        assert!(
            matches!(&unresolved, Err(VaultError::Validation(_))),
            "got {unresolved:?}"
        );

        // Choosing every field except the password is still refused: no
        // secret is ever chosen silently.
        let mut partial = std::collections::BTreeMap::new();
        partial.insert("display_name".to_string(), first_id);
        partial.insert("email".to_string(), second_id);
        let partial = manager.resolve_duplicate(
            vault.id,
            first_id,
            second_id,
            crate::DuplicateResolution::Combine {
                survivor: first_id,
                field_choices: partial,
            },
        );
        assert!(
            matches!(&partial, Err(VaultError::Validation(message)) if message.contains("password")),
            "got {partial:?}"
        );

        // A rejected Combine changes nothing at all.
        let after = serde_json::to_value(manager.get_vault(vault.id).unwrap()).unwrap();
        assert_eq!(before, after);
        assert_eq!(
            manager
                .get_item_secret(vault.id, first_id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("first-pw")
        );
        assert_eq!(
            manager
                .get_item_secret(vault.id, second_id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("second-pw")
        );
    }

    #[test]
    fn test_combine_unions_tags_ors_favourite_picks_fields_and_tombstones_the_loser() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let survivor = add_duplicate_account(
            &mut manager,
            vault.id,
            "GitHub",
            "octocat",
            "one@example.com",
            "first-pw",
            &["work"],
            false,
        );
        let other = add_duplicate_account(
            &mut manager,
            vault.id,
            "github",
            "octocat",
            "two@example.com",
            "second-pw",
            &["personal", "work"],
            true,
        );
        let (survivor_id, other_id) = (safe_item_id(&survivor), safe_item_id(&other));
        let device = manager.device_id();
        let before = manager.get_vault(vault.id).unwrap();
        let survivor_clock = before
            .items
            .iter()
            .find(|item| item.id == survivor_id)
            .unwrap()
            .clock
            .clone();
        let other_clock = before
            .items
            .iter()
            .find(|item| item.id == other_id)
            .unwrap()
            .clock
            .clone();

        let mut choices = std::collections::BTreeMap::new();
        choices.insert("display_name".to_string(), survivor_id);
        choices.insert("email".to_string(), other_id);
        choices.insert(SECRET_FIELD_PASSWORD.to_string(), other_id);

        let outcome = manager
            .resolve_duplicate(
                vault.id,
                survivor_id,
                other_id,
                crate::DuplicateResolution::Combine {
                    survivor: survivor_id,
                    field_choices: choices,
                },
            )
            .unwrap();
        assert_eq!(
            outcome,
            crate::DuplicateOutcome::Combined {
                survivor: survivor_id
            }
        );

        let vault_data = manager.get_vault(vault.id).unwrap();
        let combined = vault_data
            .items
            .iter()
            .find(|item| item.id == survivor_id)
            .unwrap();
        assert!(!combined.is_content_free());

        // Tags are a union, favourite is an OR.
        assert_eq!(
            combined.tags,
            vec!["work".to_string(), "personal".to_string()]
        );
        assert!(combined.favourite);

        // Chosen non-secret fields came from the chosen identity.
        assert_eq!(combined.display_name.as_deref(), Some("GitHub"));
        assert_eq!(
            combined.kind.as_ref().unwrap().as_account().unwrap().username,
            "octocat"
        );
        assert_eq!(
            combined.kind.as_ref().unwrap().as_account().unwrap().email.as_deref(),
            Some("two@example.com")
        );

        // The explicitly chosen secret is the one that survives, and the
        // loser keeps no secrets.
        assert_eq!(
            manager
                .get_item_secret(vault.id, survivor_id, SECRET_FIELD_PASSWORD)
                .unwrap()
                .as_deref(),
            Some("second-pw")
        );
        assert!(vault_data
            .secrets
            .iter()
            .all(|entry| entry.item_id != other_id));

        // The loser is tombstoned rather than deleted outright.
        let tombstone = vault_data
            .items
            .iter()
            .find(|item| item.id == other_id)
            .unwrap();
        assert!(tombstone.is_deleted());

        // The survivor carries a fresh tick from the resolving Device, which
        // dominates both of the clocks it was combined from.
        assert_eq!(combined.clock.compare(&survivor_clock), ClockOrder::Dominates);
        assert_eq!(combined.clock.compare(&other_clock), ClockOrder::Dominates);
        let tick = combined.clock.get(&device);
        assert!(tick > survivor_clock.get(&device));
        assert!(tick > other_clock.get(&device));
        assert_eq!(
            combined.field_clocks.get(SECRET_FIELD_PASSWORD),
            Some(&FieldVersion::new(device, tick))
        );
        assert_eq!(
            combined.field_clocks.get("tags"),
            Some(&FieldVersion::new(device, tick))
        );

        assert!(manager.duplicate_suggestions(vault.id).unwrap().is_empty());
    }

    #[test]
    fn test_duplicate_suggestions_never_cross_vaults() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir).unwrap();
        let first_vault = manager
            .create_vault("First".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let second_vault = manager
            .create_vault("Second".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let first_a = add_named_account(&mut manager, first_vault.id, "Shared", "user", "one");
        let first_b = add_named_account(&mut manager, first_vault.id, "Shared", "user", "two");
        let second_a = add_named_account(&mut manager, second_vault.id, "Shared", "user", "three");

        let first_suggestions = manager.duplicate_suggestions(first_vault.id).unwrap();
        assert_eq!(first_suggestions.len(), 1);
        assert_eq!(
            canonical_pair(
                first_suggestions[0].first.item_id,
                first_suggestions[0].second.item_id
            ),
            canonical_pair(safe_item_id(&first_a), safe_item_id(&first_b))
        );

        assert!(
            manager
                .duplicate_suggestions(second_vault.id)
                .unwrap()
                .is_empty(),
            "an Item in another vault is not a counterpart"
        );

        // And the first vault's suggestions never name the other vault's item.
        assert!(first_suggestions.iter().all(|suggestion| {
            suggestion.first.item_id != safe_item_id(&second_a)
                && suggestion.second.item_id != safe_item_id(&second_a)
        }));
    }

    fn two_managers_with_vaults(names: &[&str]) -> (VaultManager, VaultManager, Vec<VaultId>) {
        let first_dir = setup();
        let second_dir = setup();

        let mut first = VaultManager::new(first_dir.clone()).unwrap();
        let ids: Vec<VaultId> = names
            .iter()
            .map(|name| {
                first
                    .create_vault((*name).to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
                    .unwrap()
                    .id
            })
            .collect();
        first.lock_vault(None);

        let second_vaults = second_dir.join("vaults");
        std::fs::create_dir_all(&second_vaults).unwrap();
        for vault_id in &ids {
            copy_vault_file(&first_dir, &second_dir, *vault_id);
        }

        let mut second = VaultManager::new(second_dir).unwrap();
        for vault_id in &ids {
            first.unlock_vault(*vault_id, MASTER_PW, &KEY).unwrap();
            second.unlock_vault(*vault_id, MASTER_PW, &KEY).unwrap();
        }

        (first, second, ids)
    }

    /// Whole-file exchange reduced to its observable result: both managers
    /// compute the same merge and adopt it, exactly as a sync would.
    fn sync_vault(first: &mut VaultManager, second: &mut VaultManager, vault_id: VaultId) {
        let merged = match crate::merge_vaults(
            first.get_vault(vault_id).unwrap(),
            second.get_vault(vault_id).unwrap(),
        )
        .expect("both copies belong to the same vault")
        {
            crate::MergeOutcome::Merged(vault) => vault,
            crate::MergeOutcome::Conflicts(report) => {
                panic!("expected a clean sync, got {:?}", report.conflicts)
            }
        };

        first.unlocked_vaults.get_mut(&vault_id).unwrap().vault = merged.clone();
        second.unlocked_vaults.get_mut(&vault_id).unwrap().vault = merged;
    }

    fn live_items_with_id(manager: &VaultManager, item_id: ItemId) -> Vec<SafeItem> {
        manager
            .get_all_items(ItemFilter::default())
            .unwrap()
            .into_iter()
            .filter(|item| safe_item_id(item) == item_id)
            .collect()
    }

    fn safe_item_vault_id(item: &SafeItem) -> VaultId {
        match item {
            SafeItem::Account(view) => view.vault_id,
            SafeItem::Card(view) => view.vault_id,
            SafeItem::Note(view) => view.vault_id,
            SafeItem::Identity(view) => view.vault_id,
        }
    }

    fn two_managers_sharing_a_vault() -> (
        VaultManager,
        VaultManager,
        VaultId,
        PathBuf,
        PathBuf,
    ) {
        let first_dir = setup();
        let second_dir = setup();

        let mut first = VaultManager::new(first_dir.clone()).unwrap();
        let vault = first
            .create_vault("Shared".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        first.lock_vault(Some(vault.id));

        let second_vaults = second_dir.join("vaults");
        std::fs::create_dir_all(&second_vaults).unwrap();
        copy_vault_file(&first_dir, &second_dir, vault.id);

        let mut second = VaultManager::new(second_dir.clone()).unwrap();
        first.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        second.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();

        (first, second, vault.id, first_dir, second_dir)
    }

    fn copy_vault_file(from: &PathBuf, to: &PathBuf, vault_id: VaultId) {
        let source = from.join("vaults").join(format!("{}.vault", vault_id));
        let target = to.join("vaults").join(format!("{}.vault", vault_id));
        std::fs::write(target, std::fs::read(source).unwrap()).unwrap();
    }

    #[test]
    fn test_vault_metadata_edits_tick_only_the_field_they_change() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let device = manager.device_id();
        let created = manager
            .create_vault(
                "Original".to_string(),
                "#6240BF".to_string(),
                MASTER_PW,
                &KEY,
            )
            .unwrap();

        {
            let vault = manager.get_vault(created.id).unwrap();
            assert_eq!(vault.name_clock, FieldVersion::new(device, 1));
            assert_eq!(vault.color_clock, FieldVersion::new(device, 1));
            assert_eq!(vault.clock.get(&device), 1);
        }

        manager
            .update_vault(created.id, Some("Renamed".to_string()), None)
            .unwrap();
        {
            let vault = manager.get_vault(created.id).unwrap();
            assert_eq!(vault.name, "Renamed");
            assert_eq!(vault.name_clock, FieldVersion::new(device, 2));
            assert_eq!(
                vault.color_clock,
                FieldVersion::new(device, 1),
                "a rename must not restamp the colour"
            );
        }

        manager
            .update_vault(created.id, None, Some("#FF0000".to_string()))
            .unwrap();
        {
            let vault = manager.get_vault(created.id).unwrap();
            assert_eq!(vault.color, "#FF0000");
            assert_eq!(vault.name_clock, FieldVersion::new(device, 2));
            assert_eq!(vault.color_clock, FieldVersion::new(device, 3));
        }
    }

    #[test]
    fn test_vault_rename_survives_a_sync_with_the_untouched_device() {
        let (mut first, mut second, vault_id, _, _) = two_managers_sharing_a_vault();

        first
            .update_vault(vault_id, Some("Renamed".to_string()), None)
            .unwrap();
        sync_vault(&mut first, &mut second, vault_id);

        for manager in [&first, &second] {
            let vault = manager.get_vault(vault_id).unwrap();
            assert_eq!(
                vault.name, "Renamed",
                "the untouched Device must adopt the rename, not its own old pair"
            );
            assert_eq!(vault.color, "#6240BF");
        }
    }

    #[test]
    fn test_concurrent_vault_rename_and_recolour_sync_without_losing_either() {
        let (mut first, mut second, vault_id, _, _) = two_managers_sharing_a_vault();

        first
            .update_vault(vault_id, Some("Renamed".to_string()), None)
            .unwrap();
        second
            .update_vault(vault_id, None, Some("#FF0000".to_string()))
            .unwrap();

        sync_vault(&mut first, &mut second, vault_id);

        for manager in [&first, &second] {
            let vault = manager.get_vault(vault_id).unwrap();
            assert_eq!(vault.name, "Renamed");
            assert_eq!(vault.color, "#FF0000");
        }
    }

    #[test]
    fn test_concurrent_vault_renames_surface_a_vault_conflict() {
        let (mut first, mut second, vault_id, _, _) = two_managers_sharing_a_vault();

        first
            .update_vault(vault_id, Some("First name".to_string()), None)
            .unwrap();
        second
            .update_vault(vault_id, Some("Second name".to_string()), None)
            .unwrap();

        let report = match crate::merge_vaults(
            first.get_vault(vault_id).unwrap(),
            second.get_vault(vault_id).unwrap(),
        )
        .unwrap()
        {
            crate::MergeOutcome::Conflicts(report) => report,
            crate::MergeOutcome::Merged(_) => panic!("a concurrent rename must conflict"),
        };

        assert_eq!(report.conflicts.len(), 1);
        let conflict = &report.conflicts[0];
        assert!(conflict.is_vault_metadata());
        assert_eq!(conflict.field_key, "name");
        let devices: std::collections::BTreeSet<_> =
            conflict.sides.iter().map(|side| side.device).collect();
        assert_eq!(
            devices,
            std::collections::BTreeSet::from([first.device_id(), second.device_id()])
        );
    }

    #[test]
    fn test_a_dismissal_syncs_to_the_other_manager_and_silences_both() {
        let (mut first, mut second, vault_id, first_dir, second_dir) =
            two_managers_sharing_a_vault();

        let alpha_a = add_named_account(&mut first, vault_id, "Alpha", "alpha", "a1");
        let alpha_b = add_named_account(&mut first, vault_id, "Alpha", "alpha", "a2");
        let beta_a = add_named_account(&mut first, vault_id, "Beta", "beta", "b1");
        let beta_b = add_named_account(&mut first, vault_id, "Beta", "beta", "b2");
        let (alpha_a, alpha_b) = (safe_item_id(&alpha_a), safe_item_id(&alpha_b));
        let (beta_a, beta_b) = (safe_item_id(&beta_a), safe_item_id(&beta_b));

        // Whole-file exchange (ADR-0002 section 11) starts both managers from
        // the same vault content.
        first.lock_vault(Some(vault_id));
        second.lock_vault(Some(vault_id));
        copy_vault_file(&first_dir, &second_dir, vault_id);
        first.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();
        second.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();
        assert_eq!(first.duplicate_suggestions(vault_id).unwrap().len(), 2);
        assert_eq!(second.duplicate_suggestions(vault_id).unwrap().len(), 2);

        // Resolve Alpha on one Device...
        first
            .resolve_duplicate(
                vault_id,
                alpha_a,
                alpha_b,
                crate::DuplicateResolution::KeepBoth,
            )
            .unwrap();
        assert_eq!(first.duplicate_suggestions(vault_id).unwrap().len(), 1);

        // ...sync, and the other Device stops suggesting it too.
        second.lock_vault(Some(vault_id));
        copy_vault_file(&first_dir, &second_dir, vault_id);
        second.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();
        assert_eq!(second.duplicate_suggestions(vault_id).unwrap().len(), 1);
        assert!(second
            .get_vault(vault_id)
            .unwrap()
            .is_duplicate_dismissed(alpha_a, alpha_b));

        // The reverse direction works the same way.
        second
            .resolve_duplicate(
                vault_id,
                beta_a,
                beta_b,
                crate::DuplicateResolution::KeepBoth,
            )
            .unwrap();
        first.lock_vault(Some(vault_id));
        copy_vault_file(&second_dir, &first_dir, vault_id);
        first.unlock_vault(vault_id, MASTER_PW, &KEY).unwrap();

        assert!(first.duplicate_suggestions(vault_id).unwrap().is_empty());
        assert!(second.duplicate_suggestions(vault_id).unwrap().is_empty());
        for manager in [&first, &second] {
            let vault = manager.get_vault(vault_id).unwrap();
            assert!(vault.is_duplicate_dismissed(alpha_a, alpha_b));
            assert!(vault.is_duplicate_dismissed(beta_a, beta_b));
        }
    }

    #[test]
    fn test_add_card_item_with_secrets() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let card = add_test_card(&mut manager, vault.id);

        // Safe view: tagged "card", kind fields present, secrets absent
        let json = serde_json::to_value(&card).unwrap();
        assert_eq!(json["kind"], "card");
        assert_eq!(json["cardholder_name"], "Alex Doe");
        assert_eq!(json["brand"], "Visa");
        assert_eq!(json["expiry_year"], 2030);
        assert_eq!(json["card_number_last4"], "1234");

        // Which secret keys have values (never the values themselves)
        assert_eq!(
            json["secrets_present"],
            serde_json::json!(["card_number", "security_code", "pin"])
        );

        for secret_key in ["card_number", "security_code", "pin"] {
            assert!(
                json.get(secret_key).is_none(),
                "safe view must never embed the '{secret_key}' secret"
            );
        }

        // Stored with a single per-field secret entry
        let vault_ref = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault_ref.items.len(), 1);
        assert_eq!(vault_ref.secrets.len(), 1);
        assert_eq!(vault_ref.secrets[0].fields.len(), 3);

        // Each secret is retrievable individually by its field key
        let card_id = match &card {
            SafeItem::Card(c) => c.id,
            _ => panic!("expected card view"),
        };
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_CARD_NUMBER)
                .unwrap()
                .as_deref(),
            Some("4111 1111 1111 1234")
        );
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_SECURITY_CODE)
                .unwrap()
                .as_deref(),
            Some("123")
        );
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_PIN)
                .unwrap()
                .as_deref(),
            Some("4321")
        );
    }

    #[test]
    fn test_card_number_last4_derivation() {
        assert_eq!(card_number_last4("4111 1111 1111 1234"), Some("1234".into()));
        assert_eq!(card_number_last4("4111-1111-1111-1234"), Some("1234".into()));
        assert_eq!(card_number_last4("4111111111111234"), Some("1234".into()));
        assert_eq!(card_number_last4("123"), None, "too short");
        assert_eq!(card_number_last4(""), None);
        assert_eq!(card_number_last4("4111abcd"), None, "not digits");
    }

    #[test]
    fn test_card_without_secrets_has_no_present_keys_or_last4() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let card = manager
            .add_item(
                vault.id,
                Some("Bare card".to_string()),
                None,
                crate::NewItemData::Card(crate::CardData {
                    cardholder_name: "Alex".to_string(),
                    expiry_month: 5,
                    expiry_year: 2031,
                    brand: None,
                }),
                None,
            )
            .unwrap();

        assert!(
            matches!(&card, SafeItem::Card(c) if c.secrets_present.is_empty() && c.card_number_last4.is_none()),
            "got {card:?}"
        );

        let card_id = match &card {
            SafeItem::Card(c) => c.id,
            _ => panic!("expected card view"),
        };

        // Adding a secret afterwards reflects in the next safe view
        manager
            .update_item_secret(vault.id, card_id, SECRET_FIELD_PIN, Some("9999".to_string()))
            .unwrap();
        assert!(matches!(
            manager.get_item_by_id(card_id).unwrap(),
            SafeItem::Card(c) if c.secrets_present == [SECRET_FIELD_PIN]
        ));
    }

    #[test]
    fn test_get_item_secret_errors() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let card = add_test_card(&mut manager, vault.id);
        let card_id = match &card {
            SafeItem::Card(c) => c.id,
            _ => panic!("expected card view"),
        };

        // Unknown key on a card
        let unknown = manager.get_item_secret(vault.id, card_id, "cvv2");
        match unknown {
            Err(VaultError::Validation(msg)) => {
                assert!(msg.contains("Unknown secret field 'cvv2'"))
            }
            other => panic!("expected validation error, got {other:?}"),
        }

        // Secret-less kind (note) must error with a clear message
        let note = manager
            .add_item(
                vault.id,
                None,
                None,
                crate::NewItemData::Note(crate::NoteData {
                    title: None,
                    body: "body".to_string(),
                }),
                None,
            )
            .unwrap();
        let note_id = match &note {
            SafeItem::Note(n) => n.id,
            _ => panic!("expected note view"),
        };
        let secretless = manager
            .get_item_secret(vault.id, note_id, SECRET_FIELD_CARD_NUMBER)
            .unwrap_err();
        assert!(
            matches!(secretless, VaultError::Validation(ref msg) if msg.contains("no secret fields")),
            "got {secretless:?}"
        );

        // Nonexistent item
        let missing = manager.get_item_secret(vault.id, Uuid::new_v4(), SECRET_FIELD_PIN);
        assert!(matches!(missing, Err(VaultError::ItemNotFound(_))));
    }

    #[test]
    fn test_update_item_secret_updates_and_clears() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let card = add_test_card(&mut manager, vault.id);
        let card_id = match &card {
            SafeItem::Card(c) => c.id,
            _ => panic!("expected card view"),
        };

        // Set replaces the value...
        manager
            .update_item_secret(vault.id, card_id, SECRET_FIELD_PIN, Some("9999".to_string()))
            .unwrap();
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_PIN)
                .unwrap()
                .as_deref(),
            Some("9999")
        );

        // ...and None clears the field entirely
        manager
            .update_item_secret(vault.id, card_id, SECRET_FIELD_SECURITY_CODE, None)
            .unwrap();
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_SECURITY_CODE)
                .unwrap(),
            None
        );
        // Other fields of the entry survive the clear
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_CARD_NUMBER)
                .unwrap()
                .as_deref(),
            Some("4111 1111 1111 1234")
        );
        // secrets_present drops the cleared key
        assert!(matches!(
            manager.get_item_by_id(card_id).unwrap(),
            SafeItem::Card(c) if !c.secrets_present.iter().any(|k| k == SECRET_FIELD_SECURITY_CODE)
        ));

        // Unknown key rejected on update too
        assert!(manager
            .update_item_secret(vault.id, card_id, "cvv2", Some("x".to_string()))
            .is_err());

        // Persisted across unlock
        manager.lock_vault(Some(vault.id));
        manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_PIN)
                .unwrap()
                .as_deref(),
            Some("9999")
        );
    }

    #[test]
    fn test_card_move_carries_secrets() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault_a = manager
            .create_vault("A".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault_b = manager
            .create_vault("B".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let card = add_test_card(&mut manager, vault_a.id);
        let card_id = match &card {
            SafeItem::Card(c) => c.id,
            _ => panic!("expected card view"),
        };

        let moved = manager.move_item(vault_a.id, card_id, vault_b.id).unwrap();
        assert!(matches!(moved, SafeItem::Card(_)));
        assert_eq!(manager.get_vault(vault_a.id).unwrap().secrets.len(), 0);

        let moved_id = match &moved {
            SafeItem::Card(c) => c.id,
            _ => unreachable!(),
        };
        assert_eq!(
            manager
                .get_item_secret(vault_b.id, moved_id, SECRET_FIELD_CARD_NUMBER)
                .unwrap()
                .as_deref(),
            Some("4111 1111 1111 1234")
        );
        assert_eq!(
            match &moved {
                SafeItem::Card(c) => c.card_number_last4.clone(),
                _ => unreachable!(),
            },
            Some("1234".to_string())
        );
    }

    #[test]
    fn test_card_search_finds_cardholder_and_brand() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        add_test_card(&mut manager, vault.id);

        // By cardholder name
        let by_name = manager
            .get_all_items(ItemFilter {
                search_query: Some("alex doe".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_name.len(), 1);

        // By brand
        let by_brand = manager
            .get_all_items(ItemFilter {
                search_query: Some("visa".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_brand.len(), 1);

        // No false positives
        let no_match = manager
            .get_all_items(ItemFilter {
                search_query: Some("mastercard".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert!(no_match.is_empty());
    }

    #[test]
    fn test_card_expiry_month_is_validated_backend_side() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let result = manager.add_item(
            vault.id,
            None,
            None,
            crate::NewItemData::Card(crate::CardData {
                cardholder_name: "Alex".to_string(),
                expiry_month: 13,
                expiry_year: 2030,
                brand: None,
            }),
            None,
        );

        assert!(matches!(
            result,
            Err(VaultError::Validation(ref msg)) if msg.contains("between 1 and 12")
        ));
        assert_eq!(manager.get_vault(vault.id).unwrap().items.len(), 0);
    }

    #[test]
    fn test_kind_update_stamps_only_fields_that_changed() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let item = add_test_account_item(&mut manager, vault.id, "octocat", "hunter2");
        let item_id = match item {
            SafeItem::Account(view) => view.id,
            _ => panic!("expected account view"),
        };

        // Change the username only: the email was not written by this edit.
        manager
            .update_item(
                vault.id,
                item_id,
                None,
                None,
                None,
                None,
                None,
                Some(crate::NewItemData::Account(crate::AccountData {
                    username: "newcat".to_string(),
                    email: None,
                })),
            )
            .unwrap();

        let stored = manager
            .get_vault(vault.id)
            .unwrap()
            .items
            .iter()
            .find(|stored| stored.id == item_id)
            .unwrap();

        assert_eq!(stored.clock.get(&device), 2);
        assert_eq!(
            stored.field_clocks.get("username"),
            Some(&FieldVersion::new(device, 2)),
            "the changed field carries the new version"
        );
        assert_eq!(
            stored.field_clocks.get("email"),
            Some(&FieldVersion::new(device, 1)),
            "an untouched field must keep its earlier version"
        );
    }

    #[test]
    fn test_rejected_update_does_not_advance_the_clock_or_dirty_the_vault() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let card = add_test_card(&mut manager, vault.id);
        let card_id = match card {
            SafeItem::Card(view) => view.id,
            _ => panic!("expected card view"),
        };

        let (clock_before, field_clocks_before, updated_before) = {
            let stored = manager
                .get_vault(vault.id)
                .unwrap()
                .items
                .iter()
                .find(|stored| stored.id == card_id)
                .unwrap();
            (
                stored.clock.clone(),
                stored.field_clocks.clone(),
                stored.updated_at,
            )
        };
        assert!(!manager.is_dirty(vault.id));

        let rejected = manager.update_item(
            vault.id,
            card_id,
            Some("Should not apply".to_string()),
            None,
            Some(true),
            None,
            None,
            Some(crate::NewItemData::Card(crate::CardData {
                cardholder_name: "Alex Doe".to_string(),
                expiry_month: 13,
                expiry_year: 2030,
                brand: Some("Visa".to_string()),
            })),
        );

        assert!(
            matches!(rejected, Err(VaultError::Validation(ref msg)) if msg.contains("between 1 and 12")),
            "got {rejected:?}"
        );

        let stored = manager
            .get_vault(vault.id)
            .unwrap()
            .items
            .iter()
            .find(|stored| stored.id == card_id)
            .unwrap();
        assert_eq!(stored.clock, clock_before, "a failed write must not tick");
        assert_eq!(stored.clock.get(&device), 1);
        assert_eq!(stored.field_clocks, field_clocks_before);
        assert_eq!(stored.updated_at, updated_before);
        assert_eq!(stored.display_name.as_deref(), Some("Travel card"));
        assert!(!stored.favourite, "metadata must not be applied on failure");
        assert!(
            !manager.is_dirty(vault.id),
            "a rejected update must not dirty the vault"
        );
    }

    #[test]
    fn test_move_preserves_item_identity() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault_a = manager
            .create_vault("A".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault_b = manager
            .create_vault("B".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let note = add_test_note(&mut manager, vault_a.id, "Recipe", "Body");
        let note_id = match &note {
            SafeItem::Note(view) => view.id,
            _ => panic!("expected note view"),
        };
        let clock_before = manager
            .get_vault(vault_a.id)
            .unwrap()
            .items
            .iter()
            .find(|item| item.id == note_id)
            .unwrap()
            .clock
            .clone();

        let moved = manager.move_item(vault_a.id, note_id, vault_b.id).unwrap();
        let moved_id = match moved {
            SafeItem::Note(view) => view.id,
            _ => panic!("expected note view"),
        };

        assert_eq!(moved_id, note_id, "move must preserve the item's UUID");

        let moved_item = manager
            .get_vault(vault_b.id)
            .unwrap()
            .items
            .iter()
            .find(|item| item.id == moved_id)
            .unwrap();
        assert_eq!(moved_item.vault_id, vault_b.id);
        assert_eq!(
            moved_item.clock.get(&device),
            clock_before.get(&device) + 1,
            "move advances the lifecycle clock on the same identity"
        );
    }

    #[test]
    fn test_move_leaves_a_causal_record_naming_the_destination() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault_a = manager
            .create_vault("A".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault_b = manager
            .create_vault("B".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let device = manager.device_id();

        let account = add_test_account_item(&mut manager, vault_a.id, "octocat", "hunter2");
        let account_id = safe_item_id(&account);
        manager
            .move_item(vault_a.id, account_id, vault_b.id)
            .unwrap();

        // The source vault keeps a content-free record that names where the
        // item went, so a peer still holding the old copy cannot resurrect it
        // and so a move is distinguishable from a deletion.
        let source = manager.get_vault(vault_a.id).unwrap();
        let record = source
            .items
            .iter()
            .find(|item| item.id == account_id)
            .expect("the source vault keeps a move record");
        assert!(record.kind.is_none(), "a move record carries no content");
        assert!(!record.deleted, "moving out of a vault is not a deletion");
        assert_eq!(record.moved_to, Some(vault_b.id));
        assert!(record.display_name.is_none());
        assert!(record.field_clocks.is_empty());
        assert_eq!(record.clock.get(&device), 2);
        assert!(source
            .secrets
            .iter()
            .all(|entry| entry.item_id != account_id));

        // It never appears as a live item in the source vault.
        assert!(manager
            .get_all_items(ItemFilter {
                vault_id: Some(vault_a.id),
                ..Default::default()
            })
            .unwrap()
            .iter()
            .all(|item| safe_item_id(item) != account_id));
        assert!(manager
            .get_vault(vault_b.id)
            .unwrap()
            .items
            .iter()
            .any(|item| item.id == account_id && !item.is_content_free()));
    }

    #[test]
    fn test_moving_back_leaves_one_record_per_vault() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault_a = manager
            .create_vault("A".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();
        let vault_b = manager
            .create_vault("B".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let note = add_test_note(&mut manager, vault_a.id, "Recipe", "Body");
        let item_id = safe_item_id(&note);

        manager.move_item(vault_a.id, item_id, vault_b.id).unwrap();
        manager.move_item(vault_b.id, item_id, vault_a.id).unwrap();

        for vault_id in [vault_a.id, vault_b.id] {
            let vault = manager.get_vault(vault_id).unwrap();
            let records: Vec<_> = vault
                .items
                .iter()
                .filter(|item| item.id == item_id)
                .collect();
            assert_eq!(
                records.len(),
                1,
                "an identity has exactly one record per vault after moving back"
            );

            if vault_id == vault_a.id {
                assert!(!records[0].is_content_free(), "the item is live here again");
            } else {
                assert!(records[0].is_move_record());
                assert_eq!(records[0].moved_to, Some(vault_a.id));
            }
        }
        assert!(manager
            .get_vault(vault_a.id)
            .unwrap()
            .items
            .iter()
            .any(|item| item.id == item_id && !item.is_content_free()));
    }

    #[test]
    fn test_syncing_a_move_with_a_peer_holding_the_source_copy_leaves_one_live_copy() {
        let (mut first, mut second, ids) = two_managers_with_vaults(&["A", "B"]);
        let (source, destination) = (ids[0], ids[1]);

        let account = add_test_account_item(&mut first, source, "octocat", "hunter2");
        let item_id = safe_item_id(&account);

        first.move_item(source, item_id, destination).unwrap();
        // The peer still holds the pre-move live copy in the source vault.
        sync_vault(&mut first, &mut second, source);
        sync_vault(&mut first, &mut second, destination);

        for manager in [&first, &second] {
            let live = live_items_with_id(manager, item_id);
            assert_eq!(
                live.len(),
                1,
                "the same identity must not exist live in both vaults"
            );
            assert_eq!(safe_item_vault_id(&live[0]), destination);

            let source_vault = manager.get_vault(source).unwrap();
            assert!(
                source_vault.items.iter().any(|item| item.id == item_id
                    && item.is_move_record()
                    && item.moved_to == Some(destination)),
                "the source vault records where the item went"
            );
            assert!(source_vault
                .secrets
                .iter()
                .all(|entry| entry.item_id != item_id));
            assert_eq!(
                manager
                    .get_item_secret(destination, item_id, SECRET_FIELD_PASSWORD)
                    .unwrap()
                    .as_deref(),
                Some("hunter2"),
                "the secret travels with the item exactly once"
            );
        }
    }

    #[test]
    fn test_move_back_after_a_sync_converges_to_one_live_copy() {
        let (mut first, mut second, ids) = two_managers_with_vaults(&["A", "B"]);
        let (source, destination) = (ids[0], ids[1]);

        let account = add_test_account_item(&mut first, source, "octocat", "hunter2");
        let item_id = safe_item_id(&account);

        first.move_item(source, item_id, destination).unwrap();
        sync_vault(&mut first, &mut second, source);
        sync_vault(&mut first, &mut second, destination);

        // The other Device moves it back after that sync.
        second.move_item(destination, item_id, source).unwrap();
        sync_vault(&mut first, &mut second, destination);
        sync_vault(&mut first, &mut second, source);

        for manager in [&first, &second] {
            let live = live_items_with_id(manager, item_id);
            assert_eq!(live.len(), 1, "moving back must converge, not duplicate");
            assert_eq!(safe_item_vault_id(&live[0]), source);

            let destination_vault = manager.get_vault(destination).unwrap();
            assert!(
                destination_vault.items.iter().any(|item| item.id == item_id
                    && item.is_move_record()
                    && item.moved_to == Some(source)),
                "the destination records the move back"
            );
            assert!(manager
                .get_vault(source)
                .unwrap()
                .items
                .iter()
                .any(|item| item.id == item_id && !item.is_content_free()));
        }
    }

    #[test]
    fn test_moving_after_a_sync_converges_to_one_live_copy() {
        let (mut first, mut second, ids) = two_managers_with_vaults(&["A", "B", "C"]);
        let (first_vault, second_vault, third_vault) = (ids[0], ids[1], ids[2]);

        let note = add_test_note(&mut first, first_vault, "Recipe", "Body");
        let item_id = safe_item_id(&note);

        first.move_item(first_vault, item_id, second_vault).unwrap();
        sync_vault(&mut first, &mut second, first_vault);
        sync_vault(&mut first, &mut second, second_vault);

        // Move again, from the Device that learned about the first move by
        // syncing rather than by performing it.
        second
            .move_item(second_vault, item_id, third_vault)
            .unwrap();
        sync_vault(&mut first, &mut second, second_vault);
        sync_vault(&mut first, &mut second, third_vault);

        for manager in [&first, &second] {
            let live = live_items_with_id(manager, item_id);
            assert_eq!(live.len(), 1, "a second move must not duplicate");
            assert_eq!(safe_item_vault_id(&live[0]), third_vault);
            for source in [first_vault, second_vault] {
                assert!(manager
                    .get_vault(source)
                    .unwrap()
                    .items
                    .iter()
                    .all(|item| item.id != item_id || item.is_move_record()));
            }
        }
    }

    #[test]
    fn test_deleting_after_a_move_reaches_the_source_vault() {
        let (mut first, mut second, ids) = two_managers_with_vaults(&["A", "B"]);
        let (source, destination) = (ids[0], ids[1]);

        let account = add_test_account_item(&mut first, source, "octocat", "hunter2");
        let item_id = safe_item_id(&account);

        first.move_item(source, item_id, destination).unwrap();
        sync_vault(&mut first, &mut second, source);
        sync_vault(&mut first, &mut second, destination);

        // Delete it in the vault it moved to, then sync both vaults.
        first.delete_item(destination, item_id).unwrap();
        sync_vault(&mut first, &mut second, destination);
        sync_vault(&mut first, &mut second, source);

        for manager in [&first, &second] {
            assert!(
                live_items_with_id(manager, item_id).is_empty(),
                "a deletion after a move must not be undone by the source copy"
            );
            assert!(matches!(
                manager.get_item_by_id(item_id),
                Err(VaultError::ItemNotFound(_))
            ));
            assert!(
                manager
                    .get_vault(destination)
                    .unwrap()
                    .items
                    .iter()
                    .any(|item| item.id == item_id && item.is_deleted()),
                "the deletion is retained as a Tombstone in the destination"
            );
            assert!(
                manager
                    .get_vault(source)
                    .unwrap()
                    .items
                    .iter()
                    .all(|item| item.id != item_id || !item.is_deleted()),
                "the source vault's record still says 'moved', not 'deleted'"
            );
        }
    }

    #[test]
    fn test_secret_input_validation_per_kind() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Unknown secret key on a card
        let bad_key = manager.add_item(
            vault.id,
            None,
            None,
            crate::NewItemData::Card(crate::CardData {
                cardholder_name: "Alex".to_string(),
                expiry_month: 1,
                expiry_year: 2030,
                brand: None,
            }),
            Some(vec![crate::SecretInput::new("cvv2", "123".to_string())]),
        );
        assert!(matches!(bad_key, Err(VaultError::Validation(msg)) if msg.contains("Unknown secret field")));

        // Secrets sent for a secret-less kind
        let secretless = manager.add_item(
            vault.id,
            None,
            None,
            crate::NewItemData::Note(crate::NoteData {
                title: None,
                body: "body".to_string(),
            }),
            Some(vec![crate::SecretInput::new(
                SECRET_FIELD_CARD_NUMBER,
                "4111".to_string(),
            )]),
        );
        assert!(
            matches!(secretless, Err(VaultError::Validation(ref msg)) if msg.contains("no secret fields")),
            "got {secretless:?}"
        );

        assert_eq!(manager.get_vault(vault.id).unwrap().items.len(), 0);
    }
}
