use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::{Zeroize, ZeroizeOnDrop};

pub type VaultId = Uuid;
pub type ServiceId = Uuid;
pub type AccountId = Uuid;
pub type SecretId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    pub id: VaultId,
    pub name: String,
    pub services: Vec<Service>,
}

impl Vault {
    pub fn find_service(&self, service_id: ServiceId) -> Option<&Service> {
        self.services.iter().find(|s| s.id == service_id)
    }

    pub fn find_service_mut(&mut self, service_id: ServiceId) -> Option<&mut Service> {
        self.services.iter_mut().find(|s| s.id == service_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Service {
    pub id: ServiceId,
    pub name: String,
    pub accounts: Vec<Account>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: AccountId,

    pub display_name: Option<String>,
    pub username: String,
    pub email: Option<String>,

    // pub favourite: bool,
    // pub tags: Vec<String>,
    pub secret: AccountSecret,

    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct AccountSecret {
    #[zeroize(skip)]
    pub id: SecretId,
    pub password: String,
}

// ============ Safe View Models (Sent to Frontend) ============

#[derive(Debug, Clone, Serialize)]
pub struct SafeVault {
    pub id: VaultId,
    pub name: String,
    pub services: Vec<SafeService>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SafeService {
    pub id: ServiceId,
    pub name: String,
    pub accounts: Vec<SafeAccount>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SafeAccount {
    pub id: AccountId,

    pub display_name: Option<String>,
    pub username: String,
    pub email: Option<String>,

    // pub favourite: bool,
    // pub tags: Vec<String>,
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
            services: self.services.iter().map(|s| s.into_safe()).collect(),
        }
    }
}

impl IntoSafe for Service {
    type Output = SafeService;

    fn into_safe(&self) -> Self::Output {
        SafeService {
            id: self.id,
            name: self.name.clone(),
            accounts: self.accounts.iter().map(|a| a.into_safe()).collect(),
        }
    }
}

impl IntoSafe for Account {
    type Output = SafeAccount;

    fn into_safe(&self) -> Self::Output {
        SafeAccount {
            id: self.id,
            display_name: self.display_name.clone(),
            username: self.username.clone(),
            email: self.email.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}
