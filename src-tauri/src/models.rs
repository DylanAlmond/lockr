use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub type UserId = Uuid;
pub type VaultId = Uuid;
pub type ItemId = Uuid;
/// Alias for [`ItemId`]; kept until the account→item rename (ticket #27).
pub type AccountId = ItemId;
pub type SecretId = Uuid;

/// The current on-disk vault schema version.
pub const CURRENT_SCHEMA_VERSION: u32 = 2;

/// Serde default for the schema version field. Vault files written before
/// versioning omit the field (= v1) and are migrated on unlock.
fn legacy_schema_version() -> u32 {
    1
}

/// Key of the account password secret field.
pub const SECRET_FIELD_PASSWORD: &str = "password";

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
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    /// Kind-specific fields, flattened into the item on the wire
    /// (`{"kind": "account", "username": ...}`).
    #[serde(flatten)]
    pub kind: ItemKind,
}

impl VaultItem {
    /// Lowercased text of the kind's searchable fields, used by list search.
    pub fn search_text(&self) -> Vec<String> {
        let mut texts = vec![self.display_name.clone().unwrap_or_default().to_lowercase()];
        match &self.kind {
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

/// Kind-specific payload for creating or updating a non-account item.
/// Account items keep their dedicated (password-carrying) command surface.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NewItemData {
    Note(NoteData),
    Card(CardData),
    Identity(IdentityData),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountData {
    pub username: String,
    pub email: Option<String>,
}

#[allow(dead_code)] // Card items are wired up in ticket #25; the variant exists to keep the v2 schema stable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardData {
    pub cardholder_name: String,
    pub expiry_month: u8,
    pub expiry_year: u16,
    pub brand: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteData {
    pub title: Option<String>,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub fn new(item_id: ItemId, key: &str, value: String) -> Self {
        Self {
            item_id,
            fields: vec![SecretField {
                key: key.to_string(),
                value,
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

    /// Sets or replaces the given secret field's value.
    pub fn set(&mut self, key: &str, value: String) {
        if let Some(field) = self.fields.iter_mut().find(|f| f.key == key) {
            field.value = value;
        } else {
            self.fields.push(SecretField {
                key: key.to_string(),
                value,
            });
        }
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
pub struct AccountFilter {
    /// If provided, only search this specific vault. If None, search all unlocked vaults.
    pub vault_id: Option<VaultId>,

    /// If true, only return items where favourite == true
    pub favourite_only: Option<bool>,

    /// If provided, only return items that have AT LEAST ONE of these tags
    pub tags: Option<Vec<String>>,

    /// If provided, filter by search fields of the item kind containing this string
    pub search_query: Option<String>,
}

impl Default for AccountFilter {
    fn default() -> Self {
        Self {
            vault_id: None,
            favourite_only: None,
            tags: None,
            search_query: None,
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
    pub id: AccountId,

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
    /// per-field secret.
    pub fn into_versioned(&self) -> Vault {
        Vault {
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
                    kind: ItemKind::Account(AccountData {
                        username: a.username.clone(),
                        email: a.email.clone(),
                    }),
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
                    )
                })
                .collect(),
        }
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
    pub id: AccountId,
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

/// The Safe View of any item, tagged by kind for the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SafeItem {
    Account(SafeAccount),
    Note(SafeNote),
    Identity(SafeIdentity),
}

/// The Safe View of an item: only account-kind items have an account view.
/// Returns `None` for every other kind.
pub fn account_from_item(item: &VaultItem) -> Option<SafeAccount> {
    let ItemKind::Account(data) = &item.kind else {
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

/// The Safe View of an item of any sendable kind: `None` for kinds whose
/// safe view isn't wired up yet (card items can't exist in a vault yet —
/// they are rejected at creation in ticket #25's seed).
pub fn item_to_safe(item: &VaultItem) -> Option<SafeItem> {
    match &item.kind {
        ItemKind::Account(_) => account_from_item(item).map(SafeItem::Account),
        ItemKind::Note(data) => Some(SafeItem::Note(SafeNote {
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
        })),
        ItemKind::Identity(data) => Some(SafeItem::Identity(SafeIdentity {
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
        })),
        _ => None,
    }
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
            kind: ItemKind::Account(AccountData {
                username: username.to_string(),
                email: Some("dev@example.com".to_string()),
            }),
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
            )],
        };

        let bytes = serde_json::to_vec(&vault).unwrap();
        let parsed: Vault = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(parsed.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(parsed.id, vault.id);
        assert_eq!(parsed.items.len(), 1);
        let data = parsed.items[0].kind.as_account().unwrap();
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

        let vault = legacy.into_versioned();

        assert_eq!(vault.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(vault.items.len(), 1);
        let item = &vault.items[0];
        assert_eq!(item.id, account_id);
        assert_eq!(item.display_name, Some("GitHub".to_string()));
        assert_eq!(item.created_at, created);
        assert_eq!(item.updated_at, updated);
        let data = item.kind.as_account().unwrap();
        assert_eq!(data.username, "octocat");
        assert_eq!(data.email, Some("octo@example.com".to_string()));

        assert_eq!(vault.secrets.len(), 1);
        assert_eq!(vault.secrets[0].item_id, account_id);
        assert_eq!(vault.secrets[0].get(SECRET_FIELD_PASSWORD), Some("legacy_pw"));
    }
}
