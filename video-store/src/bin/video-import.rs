//! Ein-Schuss-Import in einen frischen SQLite-Store.
//!
//! video-import --pg postgres://... --data-dir video-data
//!
//! Der Zielordner darf noch nicht existieren oder muss leer sein; ein zweiter
//! Lauf auf denselben Store scheitert an den Primaerschluesseln.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use sqlite_relational_store::RelationalStore;
use video_store::{import, introspect, map};

#[derive(Parser)]
struct Cli {
    #[arg(long, env = "VIDEO_PG_URL")]
    pg: String,
    #[arg(long, default_value = "video-data")]
    data_dir: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();

    if cli.data_dir.exists() && cli.data_dir.read_dir()?.next().is_some() {
        anyhow::bail!(
            "'{}' ist nicht leer. Fuer einen neuen Lauf das Verzeichnis loeschen.",
            cli.data_dir.display()
        );
    }

    let client = introspect::connect(&cli.pg).await?;
    let catalog = introspect::read_catalog(&client).await?;
    let model = map::build_model(&catalog)?;

    let source = spec_schema::DdicSchemaSource::new(&model.activated())
        .map_err(|e| anyhow::anyhow!("Schema: {e}"))?;
    let store = RelationalStore::open(cli.data_dir.clone(), Arc::new(source)).await?;

    let report = import::import_all(&client, &store, &model).await?;
    let total: usize = report.written.iter().map(|(_, n)| n).sum();
    println!("imported {total} rows into {} sets", report.written.len());
    Ok(())
}
