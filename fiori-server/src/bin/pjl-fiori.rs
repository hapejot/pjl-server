use axum::{routing::get, Router};
use data_store::{Schema, StaticSchema};
use sqlite_relational_store::RelationalStore;
use std::{net::SocketAddr, sync::Arc};
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use fiori_server::hook::{DefaultHook, Extender};
use fiori_server::*;

async fn main_loop() -> anyhow::Result<()> {
    info!("init");
    let s = Schema::load("schema.json")?;
    // let ystore = Arc::new(odata_yaml_store::YamlStore::open("data")?);
    let store = Arc::new(RelationalStore::open(
        "rel-data",
        Arc::new(StaticSchema::new(s)),
    )?);
    let store = Extender::new(store, Arc::new(DefaultHook))?;

    let state = AppState::new(store);
    state.reload();
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
