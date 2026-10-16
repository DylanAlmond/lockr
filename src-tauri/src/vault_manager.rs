use std::{collections::HashMap, fs::read_dir, path::PathBuf};

use chrono::Utc;
use uuid::Uuid;
use zxcvbn::{zxcvbn, Entropy};

use crate::crypto::{
    decrypt_with_key, derive_master_key, encrypt_with_key, generate_salt, MasterKey, SALT_LEN,
};
use crate::{
    AccountData, AccountFilter, AccountId, CURRENT_SCHEMA_VERSION, IntoSafe, ItemKind, ItemId,
    ItemSecrets, LegacyVault, NewItemData, SecretInput, SECRET_FIELD_PASSWORD, SafeAccount,
    SafeItem, SafeVault, Vault, VaultError, VaultId, VaultItem, account_from_item,
    allowed_secret_keys, item_to_safe,
};

/// Normalizes an optional text field: empty strings mean "not set".
fn normalize(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.is_empty())
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
}

pub struct VaultManager {
    /// Where to store vault files
    vaults_dir: PathBuf,

    /// A map of decrypted unlocked vaults (with cached keys)
    unlocked_vaults: HashMap<VaultId, UnlockedVault>,

    /// When true (default), mutations save immediately.
    /// When false, mutations only mark dirty — caller must flush.
    autosave: bool,
}

impl VaultManager {
    /// Create new vault dir
    pub fn new(app_data_dir: PathBuf) -> Result<Self, VaultError> {
        let vaults_dir = app_data_dir.join("vaults");

        if !vaults_dir.exists() {
            std::fs::create_dir_all(&vaults_dir)?;
        }

        Ok(Self {
            vaults_dir,
            unlocked_vaults: HashMap::new(),
            autosave: true,
        })
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

        let vault = Vault {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: Uuid::new_v4(),
            name: name,
            color,
            items: Vec::new(),
            secrets: Vec::new(),
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
        let (vault, migrated) = match serde_json::from_slice::<Vault>(&json_bytes) {
            Ok(mut vault) => {
                // Versioned shape, but an outdated stamp (or no stamp at
                // all): re-stamp so the save brings it to the current format.
                let migrated = vault.schema_version < CURRENT_SCHEMA_VERSION;
                if migrated {
                    vault.schema_version = CURRENT_SCHEMA_VERSION;
                }
                (vault, migrated)
            }
            Err(_) => {
                // Either a pre-versioning accounts-only file or corrupt
                // content. When neither parse succeeds we surface the
                // legacy parse error; both describe the same corrupted
                // payload, and the original file is left untouched.
                let legacy: LegacyVault = serde_json::from_slice(&json_bytes)?;
                (legacy.into_versioned(), true)
            }
        };

        if migrated {
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
        std::fs::write(path, encrypted)?;

        // Clear dirty flag
        if let Some(uv) = self.unlocked_vaults.get_mut(&vault_id) {
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
    pub fn update_vault(
        &mut self,
        vault_id: VaultId,
        name: Option<String>,
        color: Option<String>,
    ) -> Result<(), VaultError> {
        {
            let vault = self.get_vault_mut(vault_id)?;
            if let Some(n) = name {
                vault.name = n;
            }
            if let Some(c) = color {
                vault.color = c;
            }
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Delete a given vault file and remove from RAM.
    pub fn delete_vault(&mut self, vault_id: VaultId) -> Result<(), VaultError> {
        if self.is_vault_unlocked(vault_id) {
            self.lock_vault(Some(vault_id));
        }

        let path = self.vault_path(vault_id);

        std::fs::remove_file(path).map_err(VaultError::Io)?;

        Ok(())
    }

    /// Create a new account item in the vault.
    pub fn add_account(
        &mut self,
        vault_id: VaultId,
        display_name: Option<String>,
        username: String,
        email: Option<String>,
        icon: Option<String>,
        password: String,
    ) -> Result<SafeAccount, VaultError> {
        let now = Utc::now();
        let item = VaultItem {
            id: Uuid::new_v4(),
            vault_id,
            display_name: normalize(display_name),
            favourite: false,
            tags: Vec::new(),
            icon,
            color: String::new(),
            created_at: now,
            updated_at: now,
            kind: ItemKind::Account(AccountData {
                username,
                email: normalize(email),
            }),
        };

        {
            let vault = self.get_vault_mut(vault_id)?;
            vault.items.push(item.clone());
            vault.secrets.push(ItemSecrets::new(
                item.id,
                SECRET_FIELD_PASSWORD,
                password,
            ));
        }

        self.maybe_save(vault_id)?;
        Ok(account_from_item(&item).expect("newly created item is an account"))
    }

    /// Update an account item.
    pub fn update_account(
        &mut self,
        vault_id: VaultId,
        account_id: AccountId,
        display_name: Option<String>,
        username: Option<String>,
        email: Option<String>,
        favourite: Option<bool>,
        tags: Option<Vec<String>>,
        icon: Option<String>,
        color: Option<String>,
        password: Option<String>,
    ) -> Result<SafeAccount, VaultError> {
        let updated = {
            let vault = self.get_vault_mut(vault_id)?;
            let Vault { items, secrets, .. } = vault;

            let item = items
                .iter_mut()
                .find(|i| i.id == account_id)
                .ok_or(VaultError::AccountNotFound(account_id.to_string()))?;
            let data = match &mut item.kind {
                ItemKind::Account(data) => data,
                _ => {
                    return Err(VaultError::Validation(format!(
                        "Item {account_id} is not an account"
                    )))
                }
            };

            if let Some(un) = username {
                data.username = un;
            }
            if let Some(fav) = favourite {
                item.favourite = fav;
            }
            if let Some(t) = tags {
                item.tags = t;
            }
            if let Some(c) = color {
                item.color = c;
            }
            if let Some(name) = display_name {
                item.display_name = normalize(Some(name));
            }
            if let Some(email) = email {
                data.email = normalize(Some(email));
            }
            if let Some(icon) = icon {
                item.icon = normalize(Some(icon));
            }

            item.updated_at = Utc::now();
            let view = account_from_item(item).expect("matched account kind");

            if let Some(pw) = password {
                let entry = secrets
                    .iter_mut()
                    .find(|s| s.item_id == account_id)
                    .ok_or_else(|| {
                        VaultError::Validation(format!(
                            "Missing secret entry for item {account_id}"
                        ))
                    })?;
                entry.set(SECRET_FIELD_PASSWORD, pw);
            }

            view
        };

        self.maybe_save(vault_id)?;
        Ok(updated)
    }

    /// Delete an account item (removes its secret entry too).
    pub fn delete_account(
        &mut self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<(), VaultError> {
        self.delete_item(vault_id, account_id)
    }

    /// Delete an item of any kind and its secret entry, if any.
    pub fn delete_item(&mut self, vault_id: VaultId, item_id: ItemId) -> Result<(), VaultError> {
        {
            let vault = self.get_vault_mut(vault_id)?;
            let exists = vault.items.iter().any(|i| i.id == item_id);
            if !exists {
                return Err(VaultError::AccountNotFound(item_id.to_string()));
            }
            vault.items.retain(|i| i.id != item_id);
            vault.secrets.retain(|s| s.item_id != item_id);
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Create a new item of a non-account kind, together with its secret
    /// fields, if the kind stores any. Secret keys are validated against the
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
                        });
                    }
                    fields
                }
                // Secret-less kinds (and omitted input) store no entry
                _ => Vec::new(),
            }
        };

        let now = Utc::now();
        let item = VaultItem {
            id: Uuid::new_v4(),
            vault_id,
            display_name: normalize(display_name),
            favourite: false,
            tags: Vec::new(),
            icon,
            color: String::new(),
            created_at: now,
            updated_at: now,
            kind,
        };

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
    /// (card items derive last4 from the vault's secret store).
    fn safe_view_of(&self, vault_id: VaultId, item_id: ItemId) -> Result<SafeItem, VaultError> {
        let vault = self.get_vault(vault_id)?;
        let item = vault
            .items
            .iter()
            .find(|i| i.id == item_id)
            .ok_or(VaultError::AccountNotFound(item_id.to_string()))?;
        Ok(item_to_safe(item, &vault.secrets))
    }

    /// Update an item's metadata, and for supported kinds its kind data.
    /// Kind data may not change an item's kind or target accounts (they keep
    /// their dedicated command surface).
    pub fn update_item(
        &mut self,
        vault_id: VaultId,
        item_id: ItemId,
        display_name: Option<String>,
        icon: Option<String>,
        favourite: Option<bool>,
        tags: Option<Vec<String>>,
        color: Option<String>,
        data: Option<NewItemData>,
    ) -> Result<SafeItem, VaultError> {
        {
            let vault = self.get_vault_mut(vault_id)?;

            let item = vault
                .items
                .iter_mut()
                .find(|i| i.id == item_id)
                .ok_or(VaultError::AccountNotFound(item_id.to_string()))?;

            if let Some(fav) = favourite {
                item.favourite = fav;
            }
            if let Some(t) = tags {
                item.tags = t;
            }
            if let Some(c) = color {
                item.color = c;
            }
            if let Some(name) = display_name {
                item.display_name = normalize(Some(name));
            }
            if let Some(icon) = icon {
                item.icon = normalize(Some(icon));
            }

            if let Some(data) = data {
                match (&mut item.kind, data) {
                    (ItemKind::Note(current), NewItemData::Note(new)) => *current = new,
                    (ItemKind::Identity(current), NewItemData::Identity(new)) => {
                        *current = new
                    }
                    (ItemKind::Card(current), NewItemData::Card(new)) => *current = new,
                    (_, _) => {
                        return Err(VaultError::Validation(
                            "Changing an item's kind is not supported (accounts keep their \
                             dedicated password command)"
                                .to_string(),
                        ))
                    }
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
            if let Some(item) = uv.vault.items.iter().find(|i| i.id == item_id) {
                return Ok(item_to_safe(item, &uv.vault.secrets));
            }
        }

        Err(VaultError::AccountNotFound(item_id.to_string()))
    }

    /// Move an item of any kind (and its secret entry, if any) to another
    /// vault as one atomic mutation. The moved item gets a fresh id.
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
            let source = self.get_vault_mut(vault_id)?;
            let idx = source
                .items
                .iter()
                .position(|i| i.id == item_id)
                .ok_or(VaultError::AccountNotFound(item_id.to_string()))?;
            let mut item = source.items.remove(idx);

            let secret = source
                .secrets
                .iter()
                .position(|s| s.item_id == item_id)
                .map(|s_idx| source.secrets.remove(s_idx));

            item.id = Uuid::new_v4();
            item.vault_id = target_vault_id;
            item.updated_at = Utc::now();

            (item, secret)
        };

        // Insert into the target vault with the secret entry remapped
        {
            let target = self.get_vault_mut(target_vault_id)?;
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
    pub fn get_all_items(&self, filter: AccountFilter) -> Result<Vec<SafeItem>, VaultError> {
        let mut results: Vec<SafeItem> = Vec::new();

        let vaults_to_scan: Vec<&Vault> = if let Some(id) = filter.vault_id {
            vec![self.get_vault(id)?]
        } else {
            self.unlocked_vaults.values().map(|uv| &uv.vault).collect()
        };

        let query = filter.search_query.map(|q| q.to_lowercase());

        for vault in vaults_to_scan {
            for item in &vault.items {
                let view = item_to_safe(item, &vault.secrets);

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

    /// Returns a specific secret field value for an item. Unknown field keys
    /// and secret-less kinds are rejected with clear validation errors.
    pub fn get_item_secret(
        &self,
        vault_id: VaultId,
        item_id: ItemId,
        field_key: &str,
    ) -> Result<String, VaultError> {
        let vault = self.get_vault(vault_id)?;

        let item = vault
            .items
            .iter()
            .find(|i| i.id == item_id)
            .ok_or(VaultError::AccountNotFound(item_id.to_string()))?;

        if allowed_secret_keys(&item.kind).is_empty() {
            return Err(VaultError::Validation(
                "This kind of item has no secret fields".to_string(),
            ));
        }

        let entry = vault
            .secrets
            .iter()
            .find(|s| s.item_id == item_id)
            .ok_or(VaultError::AccountNotFound(item_id.to_string()))?;

        entry
            .get(field_key)
            .ok_or_else(|| {
                VaultError::Validation(format!(
                    "No secret field '{field_key}' on item {item_id}"
                ))
            })
            .map(str::to_string)
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
                .find(|i| i.id == item_id)
                .ok_or(VaultError::AccountNotFound(item_id.to_string()))?;

            let allowed = allowed_secret_keys(&item.kind);
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
            let vault = self.get_vault_mut(vault_id)?;
            let item = vault
                .items
                .iter_mut()
                .find(|i| i.id == item_id)
                .expect("the item was just validated");
            item.updated_at = Utc::now();

            let entry_pos = vault.secrets.iter().position(|s| s.item_id == item_id);

            match (entry_pos, value) {
                (Some(pos), Some(value)) => vault.secrets[pos].set(field_key, value),
                (Some(pos), None) => {
                    vault.secrets[pos].remove(field_key);
                    // Drop emptied entries to keep the store tidy
                    if vault.secrets[pos].fields.is_empty() {
                        vault.secrets.remove(pos);
                    }
                }
                (None, Some(value)) => vault
                    .secrets
                    .push(ItemSecrets::new(item_id, field_key, value)),
                (None, None) => unreachable!("no-op clears return above"),
            }
        }

        self.maybe_save(vault_id)?;
        Ok(())
    }

    /// Returns only the password string for a specific account
    pub fn get_secret(
        &self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<String, VaultError> {
        self.get_item_secret(vault_id, account_id, SECRET_FIELD_PASSWORD)
    }

    /// Get the strength of a given accounts password
    pub fn get_account_password_strength(
        &self,
        vault_id: VaultId,
        account_id: AccountId,
    ) -> Result<Entropy, VaultError> {
        let pw = self.get_secret(vault_id, account_id)?;
        Ok(zxcvbn(&pw, &[]))
    }

    /// Retrieve a specific account item by id (searches all unlocked vaults)
    pub fn get_account_by_id(&self, account_id: AccountId) -> Result<SafeAccount, VaultError> {
        for uv in self.unlocked_vaults.values() {
            if let Some(item) = uv.vault.items.iter().find(|i| i.id == account_id) {
                if let Some(account) = crate::account_from_item(item) {
                    return Ok(account);
                }
            }
        }

        Err(VaultError::AccountNotFound(account_id.to_string()))
    }

    /// Retrieves account items across one or all vaults, applying filters.
    pub fn get_all_accounts(&self, filter: AccountFilter) -> Result<Vec<SafeAccount>, VaultError> {
        let mut results: Vec<SafeAccount> = Vec::new();

        // Determine which vaults to scan
        let vaults_to_scan: Vec<&Vault> = if let Some(id) = filter.vault_id {
            // Scan specific vault
            vec![self.get_vault(id)?]
        } else {
            // Scan all unlocked vaults
            self.unlocked_vaults.values().map(|uv| &uv.vault).collect()
        };

        // Prepare optional lowercase search query once for performance
        let query = filter.search_query.map(|q| q.to_lowercase());

        // Iterate and filter
        for vault in vaults_to_scan {
            for item in &vault.items {
                let Some(view) = crate::account_from_item(item) else {
                    // Non-account items are not part of the account surface
                    continue;
                };

                // Favourite Filter
                if let Some(true) = filter.favourite_only {
                    if !item.favourite {
                        continue;
                    }
                }

                // Tag Filter (Item must have AT LEAST ONE matching tag)
                if let Some(ref filter_tags) = filter.tags {
                    if !filter_tags.is_empty() {
                        let has_match = filter_tags.iter().any(|ft| item.tags.contains(ft));
                        if !has_match {
                            continue;
                        }
                    }
                }

                // Search Query Filter
                if let Some(ref q) = query {
                    let matches_username = view.username.to_lowercase().contains(q);
                    let matches_display = view
                        .display_name
                        .as_ref()
                        .map_or(false, |dn| dn.to_lowercase().contains(q));
                    let matches_email = view
                        .email
                        .as_ref()
                        .map_or(false, |em| em.to_lowercase().contains(q));

                    if !matches_username && !matches_display && !matches_email {
                        continue;
                    }
                }

                results.push(view);
            }
        }

        // Sort by most recently updated first
        results.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        Ok(results)
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
        SECRET_FIELD_CARD_NUMBER, SECRET_FIELD_PIN, SECRET_FIELD_SECURITY_CODE,
        card_number_last4,
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
        let all = manager.get_all_accounts(AccountFilter::default()).unwrap();
        assert_eq!(all.len(), 2);
        for legacy in &accounts {
            let view = all.iter().find(|a| a.id == legacy.id).unwrap();
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
            manager.get_secret(vault_id, full_id).unwrap(),
            "legacy_pw_full"
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
            manager.get_secret(vault_id, full_id).unwrap(),
            "legacy_pw_full"
        );
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
    fn test_add_account() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        // Note: no more mp/sk arguments needed!
        let account = manager
            .add_account(
                vault.id,
                Some("octocat pass".to_string()),
                "octocat".to_string(),
                None,
                None,
                "secret123".to_string(),
            )
            .unwrap();

        assert_eq!(account.display_name, Some("octocat pass".to_string()));
        assert_eq!(account.username, "octocat");
        assert_eq!(account.email, None);
        assert_eq!(manager.get_secret(vault.id, account.id).unwrap(), "secret123");
        assert_eq!(account.favourite, false);
        assert_eq!(account.tags.len(), 0);

        let vault = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault.items.len(), 1);
        assert_eq!(vault.secrets.len(), 1);
    }

    #[test]
    fn test_update_account() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "secret123".to_string(),
            )
            .unwrap();

        // No more mp/sk arguments!
        let updated = manager
            .update_account(
                vault.id,
                account.id,
                None,
                Some("octocat".to_string()),
                Some("hello@example.com".to_string()),
                Some(true),
                Some(vec!["social".to_string(), "work".to_string()]),
                None,
                None,
                Some("new_pass".to_string()),
            )
            .unwrap();

        assert_eq!(updated.username, "octocat");
        assert_eq!(updated.email, Some("hello@example.com".to_string()));
        assert_eq!(manager.get_secret(vault.id, updated.id).unwrap(), "new_pass");
        assert_eq!(updated.favourite, true);
        assert_eq!(updated.tags, vec!["social".to_string(), "work".to_string()]);
    }

    #[test]
    fn test_delete_account() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "pass".to_string(),
            )
            .unwrap();

        // No more mp/sk arguments!
        manager.delete_account(vault.id, account.id).unwrap();

        let vault = manager.get_vault(vault.id).unwrap();
        assert_eq!(vault.items.len(), 0);
        assert_eq!(vault.secrets.len(), 0);
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
            manager
                .add_account(
                    vault.id,
                    None,
                    format!("user{}", i),
                    None,
                    None,
                    format!("pass{}", i),
                )
                .unwrap();
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
            manager
                .add_account(
                    vault.id,
                    None,
                    format!("user{}", i),
                    None,
                    None,
                    format!("pass{}", i),
                )
                .unwrap();
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

        manager
            .add_account(
                vault1.id,
                None,
                "u1".to_string(),
                None,
                None,
                "p1".to_string(),
            )
            .unwrap();
        manager
            .add_account(
                vault2.id,
                None,
                "u2".to_string(),
                None,
                None,
                "p2".to_string(),
            )
            .unwrap();

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
        manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "pass".to_string(),
            )
            .unwrap();

        assert!(
            !manager.is_dirty(vault.id),
            "Autosave should have cleared dirty flag"
        );

        // Lock and re-unlock to verify persistence
        manager.lock_vault(Some(vault.id));
        let re_unlocked = manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(re_unlocked.items.len(), 1);
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

        manager
            .add_account(
                vault.id,
                Some("Old trusted".to_string()),
                "octocat".to_string(),
                Some("octo@example.com".to_string()),
                None,
                "pw".to_string(),
            )
            .unwrap();
        add_test_note(&mut manager, vault.id, "Recipe", "Pasta carbonara for two");

        let items = manager
            .get_all_items(crate::AccountFilter::default())
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

        // The account surface is unchanged: only accounts there
        let accounts = manager
            .get_all_accounts(crate::AccountFilter::default())
            .unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].username, "octocat");
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
            .get_all_items(crate::AccountFilter::default())
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

        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "pw".to_string(),
            )
            .unwrap();
        let note = add_test_note(&mut manager, vault.id, "Recipe", "Body");

        let by_id = manager.get_item_by_id(account.id).unwrap();
        assert!(matches!(by_id, crate::SafeItem::Account(_)));

        let note_id = match &note {
            crate::SafeItem::Note(n) => n.id,
            _ => panic!("expected note"),
        };
        let by_id = manager.get_item_by_id(note_id).unwrap();
        assert!(matches!(by_id, crate::SafeItem::Note(_)));

        let missing = manager.get_item_by_id(Uuid::new_v4());
        assert!(matches!(missing, Err(VaultError::AccountNotFound(_))));
    }

    #[test]
    fn test_delete_item_removes_item_and_secret_entry() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        let note = add_test_note(&mut manager, vault.id, "Note", "Body");
        let note_id = match &note {
            crate::SafeItem::Note(n) => n.id,
            _ => panic!("expected note"),
        };
        manager.delete_item(vault.id, note_id).unwrap();
        assert_eq!(manager.get_vault(vault.id).unwrap().items.len(), 0);
        assert!(manager.get_item_by_id(note_id).is_err());

        // delete_item is kind-generic: works on accounts too
        let account = manager
            .add_account(
                vault.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "pw".to_string(),
            )
            .unwrap();
        manager.delete_item(vault.id, account.id).unwrap();
        let vault_ref = manager.get_vault(vault.id).unwrap();
        assert!(vault_ref.items.is_empty());
        assert!(vault_ref.secrets.is_empty(), "Deleting must drop secrets too");
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

        let account = manager
            .add_account(
                vault_a.id,
                None,
                "octocat".to_string(),
                None,
                None,
                "hunter2".to_string(),
            )
            .unwrap();

        // Same-vault and missing-item moves are rejected
        let same = manager.move_item(vault_a.id, note_id, vault_a.id);
        assert!(matches!(same, Err(VaultError::Validation(_))));
        let missing = manager.move_item(vault_a.id, Uuid::new_v4(), vault_b.id);
        assert!(matches!(missing, Err(VaultError::AccountNotFound(_))));

        // Move the note
        let moved_note = manager.move_item(vault_a.id, note_id, vault_b.id).unwrap();
        let crate::SafeItem::Note(note_view) = &moved_note else {
            panic!("expected note view");
        };
        assert_eq!(note_view.vault_id, vault_b.id);
        assert!(note_view.favourite, "metadata must survive the move");
        assert_eq!(note_view.tags, vec!["kitchen".to_string()]);
        assert!(manager.get_item_by_id(note_id).is_err());
        assert!(manager
            .get_vault(vault_a.id)
            .unwrap()
            .items
            .iter()
            .all(|i| i.id != note_id));

        // Move the account: its secret must be remapped, not dropped
        let moved_account = manager.move_item(vault_a.id, account.id, vault_b.id).unwrap();
        let account_view = match &moved_account {
            crate::SafeItem::Account(view) => view.clone(),
            _ => panic!("expected account view"),
        };
        assert_eq!(account_view.vault_id, vault_b.id);

        let secret = manager.get_secret(vault_b.id, account_view.id).unwrap();
        assert_eq!(secret, "hunter2", "secret entry must move with the item");

        let vault_ref = manager.get_vault(vault_a.id).unwrap();
        assert!(vault_ref.items.is_empty(), "source vault is emptied out");
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
            .get_all_items(crate::AccountFilter::default())
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
                .get_all_items(crate::AccountFilter {
                    vault_id: Some(vault.id),
                    search_query: Some(query.to_string()),
                    ..Default::default()
                })
                .unwrap();
            assert_eq!(matches.len(), 1, "query '{query}' must match identity");
        }

        let misses = manager
            .get_all_items(crate::AccountFilter {
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
        assert_eq!(manager.get_vault(vault_a.id).unwrap().items.len(), 0);

        let new_id = match &moved {
            crate::SafeItem::Identity(i) => i.id,
            _ => unreachable!(),
        };
        manager.delete_item(vault_b.id, new_id).unwrap();
        assert!(manager.get_vault(vault_b.id).unwrap().items.is_empty());
    }

    #[test]
    fn test_item_search_matches_note_title_and_body() {
        let temp_dir = setup();
        let mut manager = VaultManager::new(temp_dir.clone()).unwrap();
        let vault = manager
            .create_vault("Test".to_string(), "#6240BF".to_string(), MASTER_PW, &KEY)
            .unwrap();

        manager
            .add_account(
                vault.id,
                Some("Old trusted".to_string()),
                "octocat".to_string(),
                Some("octo@example.com".to_string()),
                None,
                "pw".to_string(),
            )
            .unwrap();
        add_test_note(&mut manager, vault.id, "Recipe", "Pasta carbonara for two");

        let matches_body = manager
            .get_all_items(crate::AccountFilter {
                vault_id: Some(vault.id),
                search_query: Some("CARBONARA".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(matches_body.len(), 1, "body text must be searchable");

        let matches_title = manager
            .get_all_items(crate::AccountFilter {
                vault_id: Some(vault.id),
                search_query: Some("recipe".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(matches_title.len(), 1, "title must be searchable");

        let matches_account = manager
            .get_all_items(crate::AccountFilter {
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
            manager.get_item_secret(vault.id, card_id, SECRET_FIELD_CARD_NUMBER).unwrap(),
            "4111 1111 1111 1234"
        );
        assert_eq!(
            manager
                .get_item_secret(vault.id, card_id, SECRET_FIELD_SECURITY_CODE)
                .unwrap(),
            "123"
        );
        assert_eq!(
            manager.get_item_secret(vault.id, card_id, SECRET_FIELD_PIN).unwrap(),
            "4321"
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
            Err(VaultError::Validation(msg)) => assert!(msg.contains("No secret field 'cvv2'")),
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
        assert!(matches!(missing, Err(VaultError::AccountNotFound(_))));
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
        assert_eq!(manager.get_item_secret(vault.id, card_id, SECRET_FIELD_PIN).unwrap(), "9999");

        // ...and None clears the field entirely
        manager
            .update_item_secret(vault.id, card_id, SECRET_FIELD_SECURITY_CODE, None)
            .unwrap();
        assert!(matches!(
            manager.get_item_secret(vault.id, card_id, SECRET_FIELD_SECURITY_CODE),
            Err(VaultError::Validation(_))
        ));
        // Other fields of the entry survive the clear
        assert_eq!(
            manager.get_item_secret(vault.id, card_id, SECRET_FIELD_CARD_NUMBER).unwrap(),
            "4111 1111 1111 1234"
        );

        // Unknown key rejected on update too
        assert!(manager
            .update_item_secret(vault.id, card_id, "cvv2", Some("x".to_string()))
            .is_err());

        // Persisted across unlock
        manager.lock_vault(Some(vault.id));
        manager.unlock_vault(vault.id, MASTER_PW, &KEY).unwrap();
        assert_eq!(manager.get_item_secret(vault.id, card_id, SECRET_FIELD_PIN).unwrap(), "9999");
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
                .unwrap(),
            "4111 1111 1111 1234"
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
            .get_all_items(AccountFilter {
                search_query: Some("alex doe".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_name.len(), 1);

        // By brand
        let by_brand = manager
            .get_all_items(AccountFilter {
                search_query: Some("visa".to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(by_brand.len(), 1);

        // No false positives
        let no_match = manager
            .get_all_items(AccountFilter {
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
