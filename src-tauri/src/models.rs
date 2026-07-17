use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub type UserId = Uuid;
pub type VaultId = Uuid;
pub type AccountId = Uuid;
pub type SecretId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub color: String,
    pub icon: String,

    /// Secret Key encrypted using the user's Master Password
    pub encrypted_secret_key: String,

    /// IDs of all vaults this user owns
    pub vault_ids: Vec<VaultId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    pub id: VaultId,
    pub name: String,
    pub color: String,

    pub accounts: Vec<Account>,
}

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

#[derive(Debug, Clone, Deserialize)]
pub struct AccountFilter {
    /// If provided, only search this specific vault. If None, search all unlocked vaults.
    pub vault_id: Option<VaultId>,

    /// If true, only return accounts where favourite == true
    pub favourite_only: Option<bool>,

    /// If provided, only return accounts that have AT LEAST ONE of these tags
    pub tags: Option<Vec<String>>,

    /// If provided, filter by username, display_name, or email containing this string
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

// ============ Safe View Models (Sent to Frontend) ============

#[derive(Debug, Clone, Serialize)]
pub struct SafeVault {
    pub id: VaultId,
    pub name: String,
    pub color: String,

    pub accounts: Vec<SafeAccount>,
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
            accounts: self.accounts.iter().map(|a| a.into_safe()).collect(),
        }
    }
}

impl IntoSafe for Account {
    type Output = SafeAccount;

    fn into_safe(&self) -> Self::Output {
        SafeAccount {
            id: self.id,
            vault_id: self.vault_id,
            display_name: self.display_name.clone(),
            username: self.username.clone(),
            email: self.email.clone(),
            favourite: self.favourite,
            tags: self.tags.clone(),
            icon: self.icon.clone(),
            color: self.color.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}
