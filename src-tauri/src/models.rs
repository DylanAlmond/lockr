use std::{collections::BTreeMap, fmt};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub type UserId = Uuid;
pub type VaultId = Uuid;
pub type ItemId = Uuid;
pub type SecretId = Uuid;

/// The identity of one installation of Lockr. Minted locally on first use
/// and persisted beside the vaults directory; never derived from hardware.
pub type DeviceId = Uuid;

/// The current on-disk vault schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 3;

/// Serde default for the schema version field. Vault files written before
/// versioning omit the field (= v1) and are migrated on unlock.
fn legacy_schema_version() -> u32 {
    1
}

/// A causal version vector: the highest counter observed from each Device,
/// keyed by [`DeviceId`]. Wall clocks never participate in ordering.
///
/// Comparison is partial. Two vectors are [`ClockOrder::Equal`] when they
/// carry the same counters, one [`ClockOrder::Dominates`] the other when it
/// is strictly ahead on at least one Device and never behind, and otherwise
/// they are [`ClockOrder::Concurrent`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionVector(BTreeMap<DeviceId, u64>);

/// The causal relationship between two [`VersionVector`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockOrder {
    Equal,
    Dominates,
    Dominated,
    Concurrent,
}

impl VersionVector {
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// A vector holding exactly one observation: `device` at `counter`.
    pub fn from_version(device: DeviceId, counter: u64) -> Self {
        let mut vector = Self::new();
        if counter > 0 {
            vector.0.insert(device, counter);
        }
        vector
    }

    /// The counter observed from `device`; zero when never observed.
    pub fn get(&self, device: &DeviceId) -> u64 {
        self.0.get(device).copied().unwrap_or(0)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&DeviceId, &u64)> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The underlying map, for serialization-shaped consumers such as merge.
    pub fn as_map(&self) -> &BTreeMap<DeviceId, u64> {
        &self.0
    }

    /// Advance `device`'s counter by one and return the new value.
    pub fn increment(&mut self, device: DeviceId) -> u64 {
        let next = self.get(&device).saturating_add(1);
        self.0.insert(device, next);
        next
    }

    /// Record a field's version, keeping the greater counter if one exists.
    pub fn observe(&mut self, version: &FieldVersion) {
        if version.counter > self.get(&version.device) {
            self.0.insert(version.device, version.counter);
        }
    }

    /// Pointwise maximum with another vector.
    pub fn merge(&mut self, other: &Self) {
        for (device, counter) in &other.0 {
            if *counter > self.get(device) {
                self.0.insert(*device, *counter);
            }
        }
    }

    pub fn compare(&self, other: &Self) -> ClockOrder {
        let mut ahead = false;
        let mut behind = false;

        for device in self.0.keys().chain(other.0.keys()) {
            let mine = self.get(device);
            let theirs = other.get(device);
            if mine > theirs {
                ahead = true;
            } else if mine < theirs {
                behind = true;
            }
        }

        match (ahead, behind) {
            (false, false) => ClockOrder::Equal,
            (true, false) => ClockOrder::Dominates,
            (false, true) => ClockOrder::Dominated,
            (true, true) => ClockOrder::Concurrent,
        }
    }
}

/// The Device and counter that last wrote one field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldVersion {
    pub device: DeviceId,
    pub counter: u64,
}

impl FieldVersion {
    pub fn new(device: DeviceId, counter: u64) -> Self {
        Self { device, counter }
    }
}

impl Default for FieldVersion {
    fn default() -> Self {
        Self {
            device: Uuid::nil(),
            counter: 0,
        }
    }
}

/// Common (kind-independent) item fields that participate in per-field merge,
/// named exactly as they appear on the wire.
pub const COMMON_ITEM_FIELD_KEYS: &[&str] = &[
    "display_name",
    "favourite",
    "tags",
    "icon",
    "color",
];

/// The kind-specific field names as they appear on the wire.
pub fn kind_field_keys(kind: &ItemKind) -> &'static [&'static str] {
    match kind {
        ItemKind::Account(_) => &["username", "email"],
        ItemKind::Card(_) => &["cardholder_name", "expiry_month", "expiry_year", "brand"],
        ItemKind::Note(_) => &["title", "body"],
        ItemKind::Identity(_) => &[
            "full_name",
            "email",
            "phone",
            "address_line1",
            "address_line2",
            "city",
            "postal_code",
            "country",
        ],
    }
}

/// Every field name of an item that participates in per-field merge: common
/// fields plus the fields of its kind. Secret field keys are added by the
/// caller from the item's [`ItemSecrets`].
pub fn item_field_keys(kind: &ItemKind) -> Vec<&'static str> {
    let mut keys = COMMON_ITEM_FIELD_KEYS.to_vec();
    keys.extend_from_slice(kind_field_keys(kind));
    keys
}

/// Key of the account password secret field.
pub const SECRET_FIELD_PASSWORD: &str = "password";

/// Secret field keys of card items (per-field secrets).
pub const SECRET_FIELD_CARD_NUMBER: &str = "card_number";
pub const SECRET_FIELD_SECURITY_CODE: &str = "security_code";
pub const SECRET_FIELD_PIN: &str = "pin";

/// The secret field keys an item kind is allowed to store, in the order the
/// UI presents them. Secret-less kinds return an empty slice.
pub fn allowed_secret_keys(kind: &ItemKind) -> &'static [&'static str] {
    match kind {
        ItemKind::Account(_) => &[SECRET_FIELD_PASSWORD],
        ItemKind::Card(_) => &[
            SECRET_FIELD_CARD_NUMBER,
            SECRET_FIELD_SECURITY_CODE,
            SECRET_FIELD_PIN,
        ],
        ItemKind::Note(_) | ItemKind::Identity(_) => &[],
    }
}

/// A secret field to be stored with a newly created item, sent by the UI.
#[derive(Debug, Clone, Deserialize)]
pub struct SecretInput {
    pub key: String,
    pub value: String,
}

impl SecretInput {
    #[cfg(test)] // Test-only convenience; deserialization is the production path
    pub fn new(key: &str, value: String) -> Self {
        Self {
            key: key.to_string(),
            value,
        }
    }
}

/// Backend-derived last four digits of a card number, for list subtitles.
/// Only digits, whitespace and dashes are accepted: anything else yields
/// `None`, as does a total of fewer than four digits.
pub fn card_number_last4(card_number: &str) -> Option<String> {
    if card_number.is_empty() {
        return None;
    }

    let is_separator = |c: char| c.is_ascii_whitespace() && c != '\n' || c == '-';
    if card_number
        .chars()
        .any(|c| !c.is_ascii_digit() && !is_separator(c))
    {
        return None;
    }

    let digits: String = card_number.chars().filter(|c| c.is_ascii_digit()).collect();

    if digits.len() < 4 {
        return None;
    }

    Some(digits[digits.len() - 4..].to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub color: String,
    pub icon: Option<String>,

    /// Secret Key encrypted using the user's Master Password
    pub encrypted_secret_key: String,

    /// IDs of all vaults this user owns
    pub vault_ids: Vec<VaultId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    /// On-disk schema version. Files written before versioning omit this
    /// field; serde defaults it to 1 so `unlock_vault` knows to migrate.
    #[serde(default = "legacy_schema_version")]
    pub schema_version: u32,

    pub id: VaultId,
    pub name: String,
    pub color: String,

    /// All items in the vault, regardless of kind.
    pub items: Vec<VaultItem>,

    /// Per-field secrets for items, keyed by item id. Never sent to the UI.
    pub secrets: Vec<ItemSecrets>,
}

/// A vault entry of any kind: common metadata plus kind-specific fields.
///
/// A deleted item survives as a Tombstone: `deleted` is true, `kind` is
/// `None`, and only identity plus clock are serialized. Clocks are additive
/// so v2 content (which has neither) still parses.
#[derive(Debug, Clone)]
pub struct VaultItem {
    pub id: ItemId,

    pub vault_id: VaultId,

    pub display_name: Option<String>,
    pub favourite: bool,
    pub tags: Vec<String>,
    pub icon: Option<String>,
    pub color: String,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    /// The item's lifecycle clock: creation, move, deletion.
    pub clock: VersionVector,

    /// Last writer of each mutable wire field, keyed by its wire name.
    pub field_clocks: BTreeMap<String, FieldVersion>,

    /// True for a Tombstone. A live item always carries a `kind`.
    pub deleted: bool,

    /// Kind-specific fields, flattened into the item on the wire
    /// (`{"kind": "account", "username": ...}`). `None` for a Tombstone.
    pub kind: Option<ItemKind>,
}

impl VaultItem {
    /// A retained, content-free record of a deleted item.
    pub fn tombstone(id: ItemId, vault_id: VaultId, clock: VersionVector) -> Self {
        let epoch = DateTime::from_timestamp(0, 0).expect("unix epoch is valid");
        Self {
            id,
            vault_id,
            display_name: None,
            favourite: false,
            tags: Vec::new(),
            icon: None,
            color: String::new(),
            created_at: epoch,
            updated_at: epoch,
            clock,
            field_clocks: BTreeMap::new(),
            deleted: true,
            kind: None,
        }
    }

    pub fn is_tombstone(&self) -> bool {
        self.deleted || self.kind.is_none()
    }

    /// Stamp every mutable field of this item at one `(device, counter)`
    /// version. Used when an item is created or migrated.
    pub fn stamp_all_fields(&mut self, device: DeviceId, counter: u64) {
        self.field_clocks.clear();
        if let Some(kind) = &self.kind {
            let version = FieldVersion::new(device, counter);
            for key in item_field_keys(kind) {
                self.field_clocks.insert(key.to_string(), version);
            }
        }
    }

    pub fn record_field(&mut self, key: &str, version: FieldVersion) {
        self.field_clocks.insert(key.to_string(), version);
    }

    /// Lowercased text of the kind's searchable fields, used by list search.
    pub fn search_text(&self) -> Vec<String> {
        let Some(kind) = &self.kind else {
            return Vec::new();
        };

        let mut texts = vec![self.display_name.clone().unwrap_or_default().to_lowercase()];
        match kind {
            ItemKind::Account(data) => {
                texts.push(data.username.to_lowercase());
                if let Some(email) = &data.email {
                    texts.push(email.to_lowercase());
                }
            }
            ItemKind::Note(data) => {
                if let Some(title) = &data.title {
                    texts.push(title.to_lowercase());
                }
                texts.push(data.body.to_lowercase());
            }
            ItemKind::Card(data) => {
                texts.push(data.cardholder_name.to_lowercase());
                if let Some(brand) = &data.brand {
                    texts.push(brand.to_lowercase());
                }
            }
            ItemKind::Identity(data) => {
                texts.push(data.full_name.to_lowercase());
                for field in [
                    &data.email,
                    &data.phone,
                    &data.address_line1,
                    &data.address_line2,
                    &data.city,
                    &data.postal_code,
                    &data.country,
                ] {
                    if let Some(value) = field {
                        texts.push(value.to_lowercase());
                    }
                }
            }
        }

        texts.retain(|text| !text.is_empty());
        texts
    }
}

#[derive(Serialize)]
struct LiveVaultItemWire<'a> {
    id: &'a ItemId,
    vault_id: &'a VaultId,
    display_name: &'a Option<String>,
    favourite: bool,
    tags: &'a Vec<String>,
    icon: &'a Option<String>,
    color: &'a String,
    created_at: &'a DateTime<Utc>,
    updated_at: &'a DateTime<Utc>,
    clock: &'a VersionVector,
    field_clocks: &'a BTreeMap<String, FieldVersion>,
    #[serde(flatten)]
    kind: &'a ItemKind,
}

#[derive(Serialize)]
struct TombstoneVaultItemWire<'a> {
    id: &'a ItemId,
    vault_id: &'a VaultId,
    deleted: bool,
    clock: &'a VersionVector,
}

#[derive(Deserialize)]
struct LiveVaultItemPayload {
    id: ItemId,
    vault_id: VaultId,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    favourite: bool,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    color: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    #[serde(default)]
    clock: VersionVector,
    #[serde(default)]
    field_clocks: BTreeMap<String, FieldVersion>,
    #[serde(flatten)]
    kind: ItemKind,
}

/// A Tombstone's wire shape. `deny_unknown_fields` is deliberate: a
/// tombstone with a stray payload would otherwise lose that payload silently,
/// and a merge must resolve such a copy before serialising it.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TombstoneVaultItemPayload {
    id: ItemId,
    vault_id: VaultId,
    #[allow(dead_code)] // Presence is what matters; the branch already checked it
    deleted: bool,
    #[serde(default)]
    clock: VersionVector,
}

impl Serialize for VaultItem {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if self.is_tombstone() {
            TombstoneVaultItemWire {
                id: &self.id,
                vault_id: &self.vault_id,
                deleted: true,
                clock: &self.clock,
            }
            .serialize(serializer)
        } else {
            LiveVaultItemWire {
                id: &self.id,
                vault_id: &self.vault_id,
                display_name: &self.display_name,
                favourite: self.favourite,
                tags: &self.tags,
                icon: &self.icon,
                color: &self.color,
                created_at: &self.created_at,
                updated_at: &self.updated_at,
                clock: &self.clock,
                field_clocks: &self.field_clocks,
                kind: self
                    .kind
                    .as_ref()
                    .expect("a live item always carries a kind"),
            }
            .serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for VaultItem {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;

        let value = serde_json::Value::deserialize(deserializer)?;
        let deleted = value
            .get("deleted")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        if deleted {
            let payload: TombstoneVaultItemPayload =
                serde_json::from_value(value).map_err(D::Error::custom)?;
            Ok(VaultItem::tombstone(
                payload.id,
                payload.vault_id,
                payload.clock,
            ))
        } else {
            let payload: LiveVaultItemPayload =
                serde_json::from_value(value).map_err(D::Error::custom)?;
            Ok(VaultItem {
                id: payload.id,
                vault_id: payload.vault_id,
                display_name: payload.display_name,
                favourite: payload.favourite,
                tags: payload.tags,
                icon: payload.icon,
                color: payload.color,
                created_at: payload.created_at,
                updated_at: payload.updated_at,
                clock: payload.clock,
                field_clocks: payload.field_clocks,
                deleted: false,
                kind: Some(payload.kind),
            })
        }
    }
}

/// The kind of a [`VaultItem`] and its item-specific data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemKind {
    Account(AccountData),
    Card(CardData),
    Note(NoteData),
    Identity(IdentityData),
}

impl ItemKind {
    /// The account data if this item is an account.
    pub fn as_account(&self) -> Option<&AccountData> {
        match self {
            ItemKind::Account(data) => Some(data),
            _ => None,
        }
    }
}

/// Kind-specific payload for creating or updating an item, alongside the
/// separate initial secret fields (accounts carry their password as the
/// `password` field key).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewItemData {
    Account(AccountData),
    Note(NoteData),
    Card(CardData),
    Identity(IdentityData),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountData {
    pub username: String,
    pub email: Option<String>,
}

#[allow(dead_code)] // Brand display is wired up with card details in ticket #25
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardData {
    pub cardholder_name: String,
    pub expiry_month: u8,
    pub expiry_year: u16,
    pub brand: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NoteData {
    pub title: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityData {
    pub full_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

/// One secret field of an [`ItemSecrets`] entry.
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct SecretField {
    pub key: String,
    pub value: String,

    /// Last writer of this secret value. Secrets are fields like any other,
    /// so a concurrent password change is a Conflict.
    #[zeroize(skip)]
    #[serde(default)]
    pub clock: FieldVersion,
}

/// All secret fields of an item. Stored in the vault blob but never sent to
/// the UI except through the on-demand secret command. `Debug` is implemented
/// manually (keys only, never values).
#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct ItemSecrets {
    #[zeroize(skip)]
    pub item_id: ItemId,
    pub fields: Vec<SecretField>,
}

impl ItemSecrets {
    pub fn new(item_id: ItemId, key: &str, value: String, clock: FieldVersion) -> Self {
        Self {
            item_id,
            fields: vec![SecretField {
                key: key.to_string(),
                value,
                clock,
            }],
        }
    }

    /// The value of the given secret field, if present.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.value.as_str())
    }

    /// Sets or replaces the given secret field's value and version.
    pub fn set(&mut self, key: &str, value: String, clock: FieldVersion) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.key == key) {
            field.value = value;
            field.clock = clock;
        } else {
            self.fields.push(SecretField {
                key: key.to_string(),
                value,
                clock,
            });
        }
    }

    /// Removes the given secret field. Returns whether the key existed.
    pub fn remove(&mut self, key: &str) -> bool {
        let existed = self.fields.iter().any(|f| f.key == key);
        self.fields.retain(|f| f.key != key);
        existed
    }
}

/// Custom Debug: enum values only, never secret values (which would
/// otherwise be printed through `Vault`'s derived Debug in logs).
impl fmt::Debug for ItemSecrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let keys: Vec<&str> = self.fields.iter().map(|field| field.key.as_str()).collect();
        f.debug_struct("ItemSecrets")
            .field("item_id", &self.item_id)
            .field("fields", &keys)
            .finish()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ItemFilter {
    /// If provided, only search this specific vault. If None, search all unlocked vaults.
    pub vault_id: Option<VaultId>,

    /// If true, only return items where favourite == true
    pub favourite_only: Option<bool>,

    /// If provided, only return items that have AT LEAST ONE of these tags
    pub tags: Option<Vec<String>>,

    /// If provided, filter by search fields of the item kind containing this string
    pub search_query: Option<String>,
}

impl Default for ItemFilter {
    fn default() -> Self {
        Self {
            vault_id: None,
            favourite_only: None,
            tags: None,
            search_query: None,
        }
    }
}

impl Vault {
    /// Stamp every item and secret field in a freshly migrated vault with
    /// this Device at counter 1. Two Devices migrating the same v2 file
    /// therefore get concurrent clocks rather than identical ones.
    pub fn stamp_migrated_items(&mut self, device_id: DeviceId) {
        let version = FieldVersion::new(device_id, 1);

        for item in &mut self.items {
            item.clock = VersionVector::from_version(device_id, 1);
            if item.is_tombstone() {
                item.field_clocks.clear();
            } else {
                item.stamp_all_fields(device_id, 1);
            }
        }

        for entry in &mut self.secrets {
            for field in &mut entry.fields {
                field.clock = version;
            }

            if let Some(item) = self.items.iter_mut().find(|item| item.id == entry.item_id) {
                for field in &entry.fields {
                    item.field_clocks.insert(field.key.clone(), version);
                }
            }
        }
    }
}

// ============ Legacy (v1) accounts-only format — migration only ============

/// A pre-versioning vault file: accounts-only, passwords inline per account.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LegacyVault {
    pub id: VaultId,
    pub name: String,
    pub color: String,

    pub accounts: Vec<Account>,
}

/// An account as stored before schema versioning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: ItemId,

    pub vault_id: VaultId,

    pub display_name: Option<String>,
    pub username: String,
    pub email: Option<String>,

    pub favourite: bool,
    pub tags: Vec<String>,
    pub icon: Option<String>,
    pub color: String,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    pub secret: AccountSecret,
}

#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct AccountSecret {
    #[zeroize(skip)]
    pub id: SecretId,
    pub password: String,
}

impl LegacyVault {
    /// Transforms a v1 accounts-only vault into the versioned model: every
    /// account becomes an account-kind item and its password moves into a
    /// per-field secret. The migrated content is stamped with the local
    /// Device at counter 1.
    pub fn into_versioned(&self, device_id: DeviceId) -> Vault {
        let mut vault = Vault {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: self.id,
            name: self.name.clone(),
            color: self.color.clone(),
            items: self
                .accounts
                .iter()
                .map(|a| VaultItem {
                    id: a.id,
                    vault_id: a.vault_id,
                    display_name: a.display_name.clone(),
                    favourite: a.favourite,
                    tags: a.tags.clone(),
                    icon: a.icon.clone(),
                    color: a.color.clone(),
                    created_at: a.created_at,
                    updated_at: a.updated_at,
                    clock: VersionVector::new(),
                    field_clocks: BTreeMap::new(),
                    deleted: false,
                    kind: Some(ItemKind::Account(AccountData {
                        username: a.username.clone(),
                        email: a.email.clone(),
                    })),
                })
                .collect(),
            secrets: self
                .accounts
                .iter()
                .map(|a| {
                    ItemSecrets::new(
                        a.id,
                        SECRET_FIELD_PASSWORD,
                        a.secret.password.clone(),
                        FieldVersion::new(device_id, 1),
                    )
                })
                .collect(),
        };

        vault.stamp_migrated_items(device_id);
        vault
    }
}

// ============ Safe View Models (Sent to Frontend) ============

#[derive(Debug, Clone, Serialize)]
pub struct SafeVault {
    pub id: VaultId,
    pub name: String,
    pub color: String,
    // pub items: Vec<SafeItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SafeAccount {
    pub id: ItemId,
    pub vault_id: VaultId,

    pub display_name: Option<String>,
    pub username: String,
    pub email: Option<String>,

    pub favourite: bool,
    pub tags: Vec<String>,
    pub icon: Option<String>,
    pub color: String,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The Safe View of a note item: no secret-capable fields exist on notes.
#[derive(Debug, Clone, Serialize)]
pub struct SafeNote {
    pub id: ItemId,
    pub vault_id: VaultId,

    pub display_name: Option<String>,
    pub favourite: bool,
    pub tags: Vec<String>,
    pub icon: Option<String>,
    pub color: String,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    pub title: Option<String>,
    pub body: String,
}

/// The Safe View of an identity item: no secret-capable fields exist
/// on identities.
#[derive(Debug, Clone, Serialize)]
pub struct SafeIdentity {
    pub id: ItemId,
    pub vault_id: VaultId,

    pub display_name: Option<String>,
    pub favourite: bool,
    pub tags: Vec<String>,
    pub icon: Option<String>,
    pub color: String,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    pub full_name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address_line1: Option<String>,
    pub address_line2: Option<String>,
    pub city: Option<String>,
    pub postal_code: Option<String>,
    pub country: Option<String>,
}

/// The Safe View of a card item. Card secrets (card number, security code,
/// PIN) are never part of this view: only the backend-derived last four
/// digits of the card number are, for list subtitles.
#[derive(Debug, Clone, Serialize)]
pub struct SafeCard {
    pub id: ItemId,
    pub vault_id: VaultId,

    pub display_name: Option<String>,
    pub favourite: bool,
    pub tags: Vec<String>,
    pub icon: Option<String>,
    pub color: String,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,

    pub cardholder_name: String,
    pub brand: Option<String>,
    pub expiry_month: u8,
    pub expiry_year: u16,

    pub card_number_last4: Option<String>,

    /// Which secret field keys have values (never the values themselves),
    /// so the UI can gate copy affordances without fetching any secret.
    pub secrets_present: Vec<String>,
}

/// The Safe View of any item, tagged by kind for the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SafeItem {
    Account(SafeAccount),
    Card(SafeCard),
    Note(SafeNote),
    Identity(SafeIdentity),
}

/// The Safe View of an item: only account-kind items have an account view.
/// Returns `None` for every other kind.
pub fn account_from_item(item: &VaultItem) -> Option<SafeAccount> {
    let ItemKind::Account(data) = item.kind.as_ref()? else {
        return None;
    };

    Some(SafeAccount {
        id: item.id,
        vault_id: item.vault_id,
        display_name: item.display_name.clone(),
        username: data.username.clone(),
        email: data.email.clone(),
        favourite: item.favourite,
        tags: item.tags.clone(),
        icon: item.icon.clone(),
        color: item.color.clone(),
        created_at: item.created_at,
        updated_at: item.updated_at,
    })
}

/// The Safe View of a card item, derived from the item plus its vault's
/// secret entries (the card number lives in the secret store).
///
/// Crate-private and fallible: a Tombstone (or a non-card item) yields `None`
/// rather than panicking, so a direct consumer can never be handed content
/// that does not exist.
pub(crate) fn card_from_item(item: &VaultItem, secrets: &[ItemSecrets]) -> Option<SafeCard> {
    let kind = item.kind.as_ref()?;
    let ItemKind::Card(data) = kind else {
        return None;
    };

    let entry = secrets.iter().find(|s| s.item_id == item.id);
    let card_number = entry
        .and_then(|s| s.get(SECRET_FIELD_CARD_NUMBER))
        .and_then(card_number_last4);

    // Keys of the fields that actually have values, in presentation order
    let secrets_present = allowed_secret_keys(kind)
        .iter()
        .filter(|key| entry.is_some_and(|s| s.get(key).is_some()))
        .map(|key| key.to_string())
        .collect();

    Some(SafeCard {
        id: item.id,
        vault_id: item.vault_id,
        display_name: item.display_name.clone(),
        favourite: item.favourite,
        tags: item.tags.clone(),
        icon: item.icon.clone(),
        color: item.color.clone(),
        created_at: item.created_at,
        updated_at: item.updated_at,
        cardholder_name: data.cardholder_name.clone(),
        brand: data.brand.clone(),
        expiry_month: data.expiry_month,
        expiry_year: data.expiry_year,
        card_number_last4: card_number,
        secrets_present,
    })
}

/// The Safe View of an item of any kind. Every live kind has a safe view;
/// note and identity items carry no secret fields, account and card items
/// keep theirs out of the payload (cards only via `card_number_last4`).
/// A Tombstone has no safe view and yields `None`.
pub fn item_to_safe(item: &VaultItem, secrets: &[ItemSecrets]) -> Option<SafeItem> {
    Some(match item.kind.as_ref()? {
        ItemKind::Account(_) => {
            SafeItem::Account(account_from_item(item).expect("matched account kind"))
        }
        ItemKind::Card(_) => {
            let Some(card) = card_from_item(item, secrets) else {
                return None;
            };
            SafeItem::Card(card)
        }
        ItemKind::Note(data) => SafeItem::Note(SafeNote {
            id: item.id,
            vault_id: item.vault_id,
            display_name: item.display_name.clone(),
            favourite: item.favourite,
            tags: item.tags.clone(),
            icon: item.icon.clone(),
            color: item.color.clone(),
            created_at: item.created_at,
            updated_at: item.updated_at,
            title: data.title.clone(),
            body: data.body.clone(),
        }),
        ItemKind::Identity(data) => SafeItem::Identity(SafeIdentity {
            id: item.id,
            vault_id: item.vault_id,
            display_name: item.display_name.clone(),
            favourite: item.favourite,
            tags: item.tags.clone(),
            icon: item.icon.clone(),
            color: item.color.clone(),
            created_at: item.created_at,
            updated_at: item.updated_at,
            full_name: data.full_name.clone(),
            email: data.email.clone(),
            phone: data.phone.clone(),
            address_line1: data.address_line1.clone(),
            address_line2: data.address_line2.clone(),
            city: data.city.clone(),
            postal_code: data.postal_code.clone(),
            country: data.country.clone(),
        }),
    })
}

/// Converts our internal DB models to safe frontend models
pub trait IntoSafe {
    type Output;
    fn into_safe(&self) -> Self::Output;
}

impl IntoSafe for Vault {
    type Output = SafeVault;

    fn into_safe(&self) -> Self::Output {
        SafeVault {
            id: self.id,
            name: self.name.clone(),
            color: self.color.clone(),
            // items: self.items.iter().map(|i| i.into_safe()).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account_item(username: &str, vault_id: VaultId) -> VaultItem {
        let now = Utc::now();
        VaultItem {
            id: Uuid::new_v4(),
            vault_id,
            display_name: Some("Dev Vault".to_string()),
            favourite: true,
            tags: vec!["work".to_string()],
            icon: Some("github".to_string()),
            color: "#6240BF".to_string(),
            created_at: now,
            updated_at: now,
            clock: VersionVector::new(),
            field_clocks: BTreeMap::new(),
            deleted: false,
            kind: Some(ItemKind::Account(AccountData {
                username: username.to_string(),
                email: Some("dev@example.com".to_string()),
            })),
        }
    }

    #[test]
    fn test_v2_vault_round_trip() {
        let vault_id = Uuid::new_v4();
        let item = account_item("octocat", vault_id);
        let vault = Vault {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: vault_id,
            name: "Round Trip".to_string(),
            color: "#6240BF".to_string(),
            items: vec![item.clone()],
            secrets: vec![ItemSecrets::new(
                item.id,
                SECRET_FIELD_PASSWORD,
                "round_trip_pw".to_string(),
                FieldVersion::default(),
            )],
        };

        let bytes = serde_json::to_vec(&vault).unwrap();
        let parsed: Vault = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(parsed.id, vault.id);
        assert_eq!(parsed.items.len(), 1);
        let data = parsed.items[0].kind.as_ref().unwrap().as_account().unwrap();
        assert_eq!(data.username, "octocat");
        assert_eq!(data.email, Some("dev@example.com".to_string()));
        assert_eq!(parsed.items[0].favourite, true);
        assert_eq!(parsed.secrets[0].get(SECRET_FIELD_PASSWORD), Some("round_trip_pw"));
    }

    #[test]
    fn test_item_wire_format_is_internally_tagged() {
        let vault_id = Uuid::new_v4();
        let item = account_item("octocat", vault_id);
        let item_json: serde_json::Value = serde_json::to_value(&item).unwrap();

        assert_eq!(item_json["kind"], "account");
        assert_eq!(item_json["username"], "octocat");
        assert_eq!(item_json["display_name"], "Dev Vault");
    }

    #[test]
    fn test_vault_without_version_field_defaults_to_v1() {
        // A v2-shaped payload with the version field stripped (e.g. from a
        // broken writer) still parses; the version defaults to 1 and
        // `unlock_vault` stamps it back to the current version.
        let vault_id = Uuid::new_v4();
        let raw = format!(
            r##"{{"id": "{vault_id}", "name": "V", "color": "#6240BF", "items": [], "secrets": []}}"##
        );
        let parsed: Vault = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert!(parsed.items.is_empty());
    }

    #[test]
    fn test_v3_vault_round_trips_clocks_and_secret_versions() {
        let device = Uuid::from_u128(1);
        let vault_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();
        let now = Utc::now();

        let mut clock = VersionVector::new();
        assert_eq!(clock.increment(device), 1);
        let mut field_clocks = BTreeMap::new();
        field_clocks.insert(
            "display_name".to_string(),
            FieldVersion::new(device, 1),
        );
        field_clocks.insert("username".to_string(), FieldVersion::new(device, 1));

        let item = VaultItem {
            id: item_id,
            vault_id,
            display_name: Some("Dev Vault".to_string()),
            favourite: true,
            tags: vec!["work".to_string()],
            icon: Some("github".to_string()),
            color: "#6240BF".to_string(),
            created_at: now,
            updated_at: now,
            clock,
            field_clocks,
            deleted: false,
            kind: Some(ItemKind::Account(AccountData {
                username: "octocat".to_string(),
                email: Some("dev@example.com".to_string()),
            })),
        };

        let vault = Vault {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: vault_id,
            name: "Round Trip".to_string(),
            color: "#6240BF".to_string(),
            items: vec![item],
            secrets: vec![ItemSecrets::new(
                item_id,
                SECRET_FIELD_PASSWORD,
                "round_trip_pw".to_string(),
                FieldVersion::new(device, 1),
            )],
        };

        let bytes = serde_json::to_vec(&vault).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let device_key = device.to_string();
        assert_eq!(json["schema_version"], 3);
        assert_eq!(json["items"][0]["kind"], "account");
        assert_eq!(json["items"][0]["clock"][&device_key], 1);
        assert_eq!(
            json["items"][0]["field_clocks"]["display_name"]["device"],
            device_key
        );
        assert_eq!(
            json["items"][0]["field_clocks"]["display_name"]["counter"],
            1
        );
        assert_eq!(json["secrets"][0]["fields"][0]["clock"]["counter"], 1);

        let parsed: Vault = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed.schema_version, 3);
        let parsed_item = &parsed.items[0];
        assert_eq!(parsed_item.clock.get(&device), 1);
        assert_eq!(
            parsed_item.field_clocks.get("username"),
            Some(&FieldVersion::new(device, 1))
        );
        assert_eq!(
            parsed.secrets[0].fields[0].clock,
            FieldVersion::new(device, 1)
        );
    }

    #[test]
    fn test_v2_item_without_clocks_defaults_to_v3_empty_versions() {
        let vault_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();
        let raw = format!(
            r##"{{
                "schema_version": 2,
                "id": "{vault_id}",
                "name": "V",
                "color": "#6240BF",
                "items": [{{
                    "id": "{item_id}",
                    "vault_id": "{vault_id}",
                    "display_name": "A",
                    "favourite": false,
                    "tags": [],
                    "icon": null,
                    "color": "",
                    "created_at": "2023-11-14T22:13:20Z",
                    "updated_at": "2023-11-14T22:13:20Z",
                    "kind": "account",
                    "username": "octocat",
                    "email": null
                }}],
                "secrets": [{{
                    "item_id": "{item_id}",
                    "fields": [{{"key": "password", "value": "pw"}}]
                }}]
            }}"##
        );

        let parsed: Vault = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.schema_version, 2);
        assert!(!parsed.items[0].is_tombstone());
        assert!(parsed.items[0].clock.is_empty());
        assert!(parsed.items[0].field_clocks.is_empty());
        assert_eq!(parsed.secrets[0].fields[0].clock, FieldVersion::default());
        assert_eq!(
            parsed.items[0].kind.as_ref().unwrap().as_account().unwrap().username,
            "octocat"
        );
    }

    #[test]
    fn test_tombstone_serializes_identity_and_clock_only() {
        let device = Uuid::from_u128(7);
        let vault_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();

        let tombstone = VaultItem::tombstone(
            item_id,
            vault_id,
            VersionVector::from_version(device, 2),
        );

        assert!(tombstone.is_tombstone());
        assert!(tombstone.kind.is_none());

        let json = serde_json::to_value(&tombstone).unwrap();
        assert_eq!(json["id"], item_id.to_string());
        assert_eq!(json["vault_id"], vault_id.to_string());
        assert_eq!(json["deleted"], true);
        assert_eq!(json["clock"][&device.to_string()], 2);
        assert!(json.get("kind").is_none());
        assert!(json.get("username").is_none());
        assert!(json.get("display_name").is_none());
        assert!(json.get("tags").is_none());

        let parsed: VaultItem = serde_json::from_value(json).unwrap();
        assert!(parsed.is_tombstone());
        assert_eq!(parsed.id, item_id);
        assert_eq!(parsed.clock.get(&device), 2);
    }

    #[test]
    fn test_tombstone_with_a_stray_payload_is_rejected() {
        let vault_id = Uuid::new_v4();
        let item_id = Uuid::new_v4();

        // A tombstone that still carries content would lose it silently if
        // deserialisation accepted the extra keys. Merge must see it as
        // malformed instead.
        let raw = format!(
            r##"{{
                "id": "{item_id}",
                "vault_id": "{vault_id}",
                "deleted": true,
                "clock": {{}},
                "username": "should-not-be-here"
            }}"##
        );

        let result = serde_json::from_str::<VaultItem>(&raw);
        assert!(
            result.is_err(),
            "a tombstone carrying a payload must not deserialize"
        );
    }

    #[test]
    fn test_version_vectors_compare_causally() {
        let device_a = Uuid::from_u128(1);
        let device_b = Uuid::from_u128(2);

        let mut first = VersionVector::new();
        assert_eq!(first.increment(device_a), 1);

        let mut same = VersionVector::new();
        same.increment(device_a);
        assert_eq!(first.compare(&same), ClockOrder::Equal);

        let mut newer = same.clone();
        newer.increment(device_a);
        assert_eq!(newer.compare(&first), ClockOrder::Dominates);
        assert_eq!(first.compare(&newer), ClockOrder::Dominated);

        let mut concurrent = first.clone();
        concurrent.increment(device_b);
        assert_eq!(concurrent.compare(&newer), ClockOrder::Concurrent);
        assert_eq!(newer.compare(&concurrent), ClockOrder::Concurrent);

        let mut merged = concurrent.clone();
        merged.merge(&newer);
        assert_eq!(merged.get(&device_a), 2);
        assert_eq!(merged.get(&device_b), 1);
    }

    #[test]
    fn test_legacy_vault_migration_moves_passwords_to_field_secrets() {
        let vault_id = Uuid::new_v4();
        let account_id = Uuid::new_v4();
        let created = DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let updated = DateTime::from_timestamp(1_701_000_000, 0).unwrap();
        let legacy = LegacyVault {
            id: vault_id,
            name: "Legacy".to_string(),
            color: "#6240BF".to_string(),
            accounts: vec![Account {
                id: account_id,
                vault_id,
                display_name: Some("GitHub".to_string()),
                username: "octocat".to_string(),
                email: Some("octo@example.com".to_string()),
                favourite: true,
                tags: vec!["dev".to_string()],
                icon: Some("github".to_string()),
                color: "#6240BF".to_string(),
                created_at: created,
                updated_at: updated,
                secret: AccountSecret {
                    id: Uuid::new_v4(),
                    password: "legacy_pw".to_string(),
                },
            }],
        };

        let device = Uuid::from_u128(42);
        let vault = legacy.into_versioned(device);

        assert_eq!(vault.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(vault.items.len(), 1);
        let item = &vault.items[0];
        assert_eq!(item.id, account_id);
        assert_eq!(item.display_name, Some("GitHub".to_string()));
        assert_eq!(item.created_at, created);
        assert_eq!(item.updated_at, updated);
        let data = item.kind.as_ref().unwrap().as_account().unwrap();
        assert_eq!(data.username, "octocat");
        assert_eq!(data.email, Some("octo@example.com".to_string()));

        // The v1 content was stamped by the local Device at counter 1
        assert_eq!(item.clock.get(&device), 1);
        assert_eq!(
            item.field_clocks.get("password"),
            Some(&FieldVersion::new(device, 1))
        );

        assert_eq!(vault.secrets.len(), 1);
        assert_eq!(vault.secrets[0].item_id, account_id);
        assert_eq!(vault.secrets[0].get(SECRET_FIELD_PASSWORD), Some("legacy_pw"));
        assert_eq!(
            vault.secrets[0].fields[0].clock,
            FieldVersion::new(device, 1)
        );
    }
}
