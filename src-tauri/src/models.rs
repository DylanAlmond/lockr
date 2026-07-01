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

impl Vault {
    pub fn find_service(&self, service_id: ServiceId) -> Option<&Service> {
        self.services.iter().find(|s| s.id == service_id)
    }

    pub fn find_service_mut(&mut self, service_id: ServiceId) -> Option<&mut Service> {
        self.services.iter_mut().find(|s| s.id == service_id)
    }
}
