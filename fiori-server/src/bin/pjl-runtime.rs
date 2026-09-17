//! Laufzeitserver: fuehrt ein exportiertes Data Dictionary aus.
//!
//! Kennt das Metamodell nicht. Er liest `ddic.json` (und optional
//! `schema.json`) aus dem Exportverzeichnis des Designers, legt fehlende
//! Tabellen an und stellt fuer die dort definierten Apps OData-Service,
//! `$metadata` und Launchpad bereit.
//!
//!     cargo run --bin pjl-runtime -- --ddic export/ddic.json

use fiori_server::config::{ServerConfig, ServerMode};
use fiori_server::server;
use tracing::{error, info};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let config = ServerConfig::parse(ServerMode::Runtime);
    match server::run(config).await {
        Ok(()) => {
            info!("runtime finished");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            error!("runtime shutdown with error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
