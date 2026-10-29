//! Legt das Speicherschema der Runtime aus `ddic.json` in einer Datenbank an.
//!
//! video-schema --ddic ddic.json --target-pg postgres://...
//!
//! Dieselbe Schemaerzeugung wie die Runtime beim Start (PostgresStore::connect),
//! aber ohne Server. Die Zieldatenbank muss leer sein (Migrations-Datenbank).

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use postgres_relational_store::PostgresStore;
use spec::data_dictionary::DataDictionary;
use spec_schema::DdicSchemaSource;

#[derive(Parser)]
struct Cli {
    /// Zielmodell (von Hand gepflegt).
    #[arg(long, default_value = "ddic.json")]
    ddic: PathBuf,
    /// Zieldatenbank, in der das Schema angelegt wird.
    #[arg(long, env = "TARGET_PG")]
    target_pg: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();

    let file = std::fs::File::open(&cli.ddic)?;
    let mut ddic: DataDictionary = serde_json::from_reader(std::io::BufReader::new(file))?;
    ddic.activate();

    let source = DdicSchemaSource::new(&ddic).map_err(|e| anyhow::anyhow!("Schema: {e}"))?;
    let _store = PostgresStore::connect(&cli.target_pg, Arc::new(source)).await?;
    println!("schema applied from {}", cli.ddic.display());
    Ok(())
}
