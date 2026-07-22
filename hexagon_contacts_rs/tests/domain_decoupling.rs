//! Beweis: die Domain-Logik lässt sich vollständig ohne Postgres testen,
//! weil ContactService nur das ContactRepository-Trait kennt.
use std::sync::{Arc, Mutex};
use std::collections::HashMap;

use async_trait::async_trait;
use hexagon_contacts_rs::domain::entity::Contact;
use hexagon_contacts_rs::domain::error::DomainError;
use hexagon_contacts_rs::domain::service::ContactService;
use hexagon_contacts_rs::port::inbound::contact_port::ContactUseCase;
use hexagon_contacts_rs::port::outbound::contact_repository::ContactRepository;

struct InMemoryContactRepository {
    data: Mutex<HashMap<i32, Contact>>,
    next_id: Mutex<i32>,
}

impl InMemoryContactRepository {
    fn new() -> Self {
        Self { data: Mutex::new(HashMap::new()), next_id: Mutex::new(1) }
    }
}

#[async_trait]
impl ContactRepository for InMemoryContactRepository {
    async fn save(&self, mut contact: Contact) -> Result<Contact, DomainError> {
        let mut next_id = self.next_id.lock().unwrap();
        contact.id = Some(*next_id);
        self.data.lock().unwrap().insert(*next_id, contact.clone());
        *next_id += 1;
        Ok(contact)
    }
    async fn find_by_id(&self, id: i32) -> Result<Option<Contact>, DomainError> {
        Ok(self.data.lock().unwrap().get(&id).cloned())
    }
    async fn find_by_customer(&self, customer_id: &str) -> Result<Vec<Contact>, DomainError> {
        Ok(self.data.lock().unwrap().values().filter(|c| c.customer_id == customer_id).cloned().collect())
    }
    async fn delete(&self, id: i32) -> Result<(), DomainError> {
        self.data.lock().unwrap().remove(&id);
        Ok(())
    }
}

#[tokio::test]
async fn contact_lifecycle_works_without_postgres() {
    let repo: Arc<dyn ContactRepository> = Arc::new(InMemoryContactRepository::new());
    let service = ContactService::new(repo);

    let created = service
        .create_contact("K123".into(), "Max Mustermann".into(), "max@example.com".into(), "0221-123456".into())
        .await
        .expect("Kontakt anlegen sollte funktionieren");
    assert_eq!(created.name, "Max Mustermann");

    let fetched = service.get_contact(created.id.unwrap()).await.unwrap();
    assert_eq!(fetched.email, "max@example.com");

    let list = service.list_contacts_for_customer("K123").await.unwrap();
    assert_eq!(list.len(), 1);

    let invalid = service
        .create_contact("K123".into(), "Fehler".into(), "keine-email".into(), "".into())
        .await;
    assert!(matches!(invalid, Err(DomainError::Validation(_))));

    service.delete_contact(created.id.unwrap()).await.unwrap();
    let list_after = service.list_contacts_for_customer("K123").await.unwrap();
    assert_eq!(list_after.len(), 0);
}
