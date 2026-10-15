//! TN3270-Server fuer den video-store OData-Service.
//!
//! Umgebungsvariablen:
//!   VIDEO3270_BIND  Bind-Adresse (Standard 0.0.0.0:3270)
//!   ODATA_URL       OData-Basis (Standard http://nuc:3035/Service)
//!   ODATA_USER      Wert fuer den Header X-Forwarded-User (Standard 3270)
//!   RUST_LOG        Log-Filter (Standard video_3270=info); z.B. RUST_LOG=debug

mod context;
mod odata;
mod screen;
mod session;
mod tn3270;
mod views;

use anyhow::Result;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("video_3270=info")),
        )
        .init();

    let bind = env_or("VIDEO3270_BIND", "0.0.0.0:3270");
    let odata = odata::Odata::new(
        env_or("ODATA_URL", "http://nuc:3035/Service"),
        env_or("ODATA_USER", "3270"),
    )?;
    let listener = TcpListener::bind(&bind).await?;
    tracing::info!(%bind, "video-3270 laeuft");
    loop {
        let (sock, peer) = listener.accept().await?;
        let odata = odata.clone();
        tokio::spawn(async move {
            tracing::info!(%peer, "Verbindung");
            if let Err(e) = session::run(sock, odata).await {
                tracing::warn!(%peer, error = %format!("{e:#}"), "Sitzung mit Fehler beendet");
            }
            tracing::info!(%peer, "Verbindung beendet");
        });
    }
}
