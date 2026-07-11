use axum::{Router, routing::get};
use std::net::SocketAddr;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use fiori_server::*;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(fmt::layer()) // hübsche Konsolenausgabe
        .with(EnvFilter::from_default_env()) // Steuerung über RUST_LOG
        .init();

    let app = Router::new()
        // .nest("/users", services::users::router())
        .nest("/Service/", batch::router(srv::router()))
        .nest("/flp/", flp::router())
        .route("/", get(flp::index))
        .fallback_service(ServeDir::new("web"))
        .layer(TraceLayer::new_for_http());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    info!("Läuft auf http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
