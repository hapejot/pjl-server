//! Erzeugt `ddic.json` aus dem Katalog der Quelldatenbank.
//!
//! video-ddic --pg postgres://user:pw@host:5432/db --out ddic.json

use std::path::PathBuf;

use clap::Parser;
use video_store::{introspect, map};

#[derive(Parser)]
struct Cli {
    /// Verbindungs-URL der Quelldatenbank (nur lesend genutzt).
    #[arg(long, env = "VIDEO_PG_URL")]
    pg: String,
    /// Zieldatei fuer das Data Dictionary.
    #[arg(long, default_value = "video-store/ddic.json")]
    out: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();

    let client = introspect::connect(&cli.pg).await?;
    let catalog = introspect::read_catalog(&client).await?;
    let model = map::build_model(&catalog)?;

    let file = std::fs::File::create(&cli.out)?;
    serde_json::to_writer_pretty(std::io::BufWriter::new(file), &model.ddic)?;

    println!(
        "{} tables, {} relationships -> {}",
        model.tables.len(),
        model.ddic.relationships().count(),
        cli.out.display()
    );
    Ok(())
}
