use async_trait::async_trait;

use crate::domain::entity::Contact;
use crate::domain::error::DomainError;

/// Outbound port. Implemented by adapters (Postgres, in-memory fake, ...).
/// The domain depends on this trait only - never on a concrete database.
#[async_trait]
pub trait ContactRepository: Send + Sync {
    async fn save(&self, contact: Contact) -> Result<Contact, DomainError>;
    async fn find_by_id(&self, contact_id: i32) -> Result<Option<Contact>, DomainError>;
    async fn find_by_customer(&self, customer_id: &str) -> Result<Vec<Contact>, DomainError>;
    async fn delete(&self, contact_id: i32) -> Result<(), DomainError>;
}
