use std::sync::Arc;

use async_trait::async_trait;
use regex::Regex;

use crate::domain::entity::Contact;
use crate::domain::error::DomainError;
use crate::port::inbound::contact_port::ContactUseCase;
use crate::port::outbound::contact_repository::ContactRepository;

/// Application/domain logic. Knows nothing about OData or Postgres -
/// only about the ContactRepository trait object it was handed.
pub struct ContactService {
    repository: Arc<dyn ContactRepository>,
}

impl ContactService {
    pub fn new(repository: Arc<dyn ContactRepository>) -> Self {
        Self { repository }
    }

    fn validate_email(email: &str) -> Result<(), DomainError> {
        let pattern = Regex::new(r"^[^@\s]+@[^@\s]+\.[^@\s]+$").unwrap();
        if pattern.is_match(email) {
            Ok(())
        } else {
            Err(DomainError::Validation(format!("Invalid email address: {email}")))
        }
    }
}

#[async_trait]
impl ContactUseCase for ContactService {
    async fn create_contact(
        &self,
        customer_id: String,
        name: String,
        email: String,
        phone: String,
    ) -> Result<Contact, DomainError> {
        Self::validate_email(&email)?;
        let trimmed_name = name.trim().to_string();
        if trimmed_name.is_empty() {
            return Err(DomainError::Validation("Contact name must not be empty".into()));
        }

        let contact = Contact {
            id: None,
            customer_id,
            name: trimmed_name,
            email,
            phone,
        };
        self.repository.save(contact).await
    }

    async fn get_contact(&self, contact_id: i32) -> Result<Contact, DomainError> {
        self.repository
            .find_by_id(contact_id)
            .await?
            .ok_or_else(|| DomainError::NotFound(format!("Contact {contact_id} not found")))
    }

    async fn list_contacts_for_customer(&self, customer_id: &str) -> Result<Vec<Contact>, DomainError> {
        self.repository.find_by_customer(customer_id).await
    }

    async fn delete_contact(&self, contact_id: i32) -> Result<(), DomainError> {
        if self.repository.find_by_id(contact_id).await?.is_none() {
            return Err(DomainError::NotFound(format!("Contact {contact_id} not found")));
        }
        self.repository.delete(contact_id).await
    }
}
