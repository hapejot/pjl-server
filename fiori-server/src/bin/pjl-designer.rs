//! Designer-Server: pflegt das Data Dictionary.
//!
//! Zeigt in seinem Launchpad ausschliesslich die Pflege-Apps des Metamodells
//! (Packages, Entities, Fields, Relations, Apps). Was hier modelliert wird,
//! verlaesst den Server ueber `POST /Service/export` - ausgeloest vom Button
//! "Publish Dictionary" - als `ddic.json` + `schema.json`.
//!
//!     cargo run --bin pjl-designer -- --export-dir export

use fiori_server::config::{ServerConfig, ServerMode};
use fiori_server::server;
use tracing::{error, info};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> std::process::ExitCode {
    tracing_subscriber::registry()
        .with(fmt::layer().with_file(true).with_line_number(true))
        .with(EnvFilter::from_default_env())
        .init();

    let config = ServerConfig::parse(ServerMode::Designer);
    match server::run(config).await {
        Ok(()) => {
            info!("designer finished");
            std::process::ExitCode::SUCCESS
        }
        Err(e) => {
            error!("designer shutdown with error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}
