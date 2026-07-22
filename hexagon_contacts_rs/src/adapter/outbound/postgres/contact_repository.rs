use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_postgres::{Client, NoTls};

use crate::domain::entity::Contact;
use crate::domain::error::DomainError;
use crate::port::outbound::contact_repository::ContactRepository;

pub struct PostgresContactRepository {
    client: Mutex<Client>,
}

impl PostgresContactRepository {
    pub async fn connect(config: &str) -> Result<Self, DomainError> {
        let (client, connection) = tokio_postgres::connect(config, NoTls)
            .await
            .map_err(|e| DomainError::Repository(e.to_string()))?;

        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Postgres connection error: {e}");
            }
        });

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS contacts (
                    id SERIAL PRIMARY KEY,
                    customer_id TEXT NOT NULL,
                    name TEXT NOT NULL,
                    email TEXT NOT NULL,
                    phone TEXT DEFAULT ''
                )",
                &[],
            )
            .await
            .map_err(|e| DomainError::Repository(e.to_string()))?;

        Ok(Self { client: Mutex::new(client) })
    }

    fn row_to_contact(row: &tokio_postgres::Row) -> Contact {
        Contact {
            id: Some(row.get("id")),
            customer_id: row.get("customer_id"),
            name: row.get("name"),
            email: row.get("email"),
            phone: row.get("phone"),
        }
    }
}

#[async_trait]
impl ContactRepository for PostgresContactRepository {
    async fn save(&self, contact: Contact) -> Result<Contact, DomainError> {
        let client = self.client.lock().await;
        let row = client
            .query_one(
                "INSERT INTO contacts (customer_id, name, email, phone)
                 VALUES ($1, $2, $3, $4) RETURNING id",
                &[&contact.customer_id, &contact.name, &contact.email, &contact.phone],
            )
            .await
            .map_err(|e| DomainError::Repository(e.to_string()))?;
        Ok(Contact { id: Some(row.get("id")), ..contact })
    }

    async fn find_by_id(&self, contact_id: i32) -> Result<Option<Contact>, DomainError> {
        let client = self.client.lock().await;
        let row = client
            .query_opt("SELECT * FROM contacts WHERE id = $1", &[&contact_id])
            .await
            .map_err(|e| DomainError::Repository(e.to_string()))?;
        Ok(row.as_ref().map(Self::row_to_contact))
    }

    async fn find_by_customer(&self, customer_id: &str) -> Result<Vec<Contact>, DomainError> {
        let client = self.client.lock().await;
        let rows = client
            .query(
                "SELECT * FROM contacts WHERE customer_id = $1 ORDER BY id",
                &[&customer_id],
            )
            .await
            .map_err(|e| DomainError::Repository(e.to_string()))?;
        Ok(rows.iter().map(Self::row_to_contact).collect())
    }

    async fn delete(&self, contact_id: i32) -> Result<(), DomainError> {
        let client = self.client.lock().await;
        client
            .execute("DELETE FROM contacts WHERE id = $1", &[&contact_id])
            .await
            .map_err(|e| DomainError::Repository(e.to_string()))?;
        Ok(())
    }
}
