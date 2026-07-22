use async_trait::async_trait;

use crate::domain::entity::Contact;
use crate::domain::error::DomainError;

/// Inbound port. Implemented by the domain service, called by adapters
/// (OData, CLI, a future gRPC endpoint, ...). Adapters depend on this
/// trait only - never on the concrete ContactService.
#[async_trait]
pub trait ContactUseCase: Send + Sync {
    async fn create_contact(
        &self,
        customer_id: String,
        name: String,
        email: String,
        phone: String,
    ) -> Result<Contact, DomainError>;

    async fn get_contact(&self, contact_id: i32) -> Result<Contact, DomainError>;

    async fn list_contacts_for_customer(&self, customer_id: &str) -> Result<Vec<Contact>, DomainError>;

    async fn delete_contact(&self, contact_id: i32) -> Result<(), DomainError>;
}
