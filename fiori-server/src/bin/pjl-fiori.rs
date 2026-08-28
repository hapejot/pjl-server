use axum::{Router, routing::get};
use odata_yaml_store::{EntityMetadata, YamlStore};
use spec::data_dictionary::DataDictonary;
use std::{net::SocketAddr, sync::Arc};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use fiori_server::*;

async fn main_loop() -> anyhow::Result<()> {
    info!("init");
    let ddic = Arc::new(DataDictonary::new());
    let mut metadata = EntityMetadata::new();
    for entity in ddic.entities() {
        metadata.register_key(&entity.setname(), &["ID"]);
    }
    let store = Arc::new(YamlStore::open("data", metadata)?);
    let state = AppState { store, ddic };
    let app = Router::new()
        // .nest("/users", services::users::router())
        .nest("/Service/", batch::router(srv::router(), state.clone()))
        .nest("/flp/", flp::router())
        .route("/apps/{app}/Component.js", get(flp::app_component_handler))
        .route("/apps/{app}/manifest.json", get(flp::app_manifest_handler))
        // .route("/apps/{app}/{*path}", get(flp::app_handler))
        .route("/", get(flp::index))
        .fallback_service(ServeDir::new("web"))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3033));
    info!("Startet auf http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("läuft");
    axum::serve(listener, app).await?;
    Ok(())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer()) // hübsche Konsolenausgabe
        .with(EnvFilter::from_default_env()) // Steuerung über RUST_LOG
        .init();
    match main_loop().await {
        Ok(_) => info!("Server finished without error."),
        Err(e) => error!("Server shutdown with error: {}", e),
    }
}
