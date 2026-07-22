//! Composition root. This is the only place that knows about BOTH the
//! concrete adapters AND the domain service - everywhere else only sees
//! traits (ports).
use std::sync::Arc;

use hexagon_contacts_rs::adapter::inbound::odata::contact_odata_adapter::create_odata_app;
use hexagon_contacts_rs::adapter::outbound::postgres::contact_repository::PostgresContactRepository;
use hexagon_contacts_rs::domain::service::ContactService;
use hexagon_contacts_rs::port::inbound::contact_port::ContactUseCase;
use hexagon_contacts_rs::port::outbound::contact_repository::ContactRepository;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dsn = std::env::var("CONTACTS_DSN")
        .unwrap_or_else(|_| "host=localhost user=postgres password=postgres dbname=contacts".into());

    // 1. Outbound adapter - implements the outbound port
    let repository: Arc<dyn ContactRepository> =
        Arc::new(PostgresContactRepository::connect(&dsn).await?);

    // 2. Domain service - implements the inbound port, depends only on the
    //    outbound port (repository trait object), not on Postgres directly
    let contact_service: Arc<dyn ContactUseCase> = Arc::new(ContactService::new(repository));

    // 3. Inbound adapter - calls the inbound port
    let app = create_odata_app(contact_service);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:5000").await?;
    println!("Listening on http://0.0.0.0:5000");
    axum::serve(listener, app).await?;
    Ok(())
}
