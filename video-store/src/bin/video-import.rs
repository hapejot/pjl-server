//! Ein-Schuss-Import der Video-Datenbank in den PostgreSQL-Store der Anwendung.
//!
//! video-import --pg postgres://...(Quelle, nur lesend) \
//!              --target-pg postgres://...(Ziel, eigene Instanz)
//!
//! Das Ziel muss leer sein. Mit `--reset` wird es vorher geleert (Schema
//! `public` wird neu angelegt), mit `--clear-only` nur geleert und beendet.
//! Das Leeren betrifft ausschliesslich die Ziel-Datenbank, nie die Quelle.

use std::sync::Arc;

use clap::Parser;
use postgres_relational_store::PostgresStore;
use video_store::{import, introspect, map};

#[derive(Parser)]
struct Cli {
    /// Quelle: die bestehende Video-Datenbank. Wird nur lesend benutzt.
    #[arg(long, env = "VIDEO_PG_URL", required_unless_present = "clear_only")]
    pg: Option<String>,
    /// Ziel: PostgreSQL-Store der Video-Anwendung.
    #[arg(long, env = "STORE_PG_URL")]
    target_pg: String,
    /// Vorhandene Daten im Ziel vor dem Import verwerfen.
    #[arg(long)]
    reset: bool,
    /// Nur das Ziel leeren und beenden (Aufraeumen nach einem Fehler).
    #[arg(long)]
    clear_only: bool,
}

/// Leert das Ziel: das Schema `public` wird verworfen und neu angelegt.
async fn clear_target(target: &tokio_postgres::Client) -> anyhow::Result<()> {
    target
        .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
        .await?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let cli = Cli::parse();

    let target = introspect::connect(&cli.target_pg).await?;
    if cli.clear_only {
        clear_target(&target).await?;
        println!("target cleared");
        return Ok(());
    }
    let Some(pg) = cli.pg.as_deref() else {
        anyhow::bail!("--pg (VIDEO_PG_URL) fehlt");
    };

    let existing: i64 = target
        .query_one(
            "SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = current_schema()",
            &[],
        )
        .await?
        .get(0);
    if existing > 0 {
        if !cli.reset {
            anyhow::bail!(
                "das Ziel enthaelt bereits {existing} Tabellen. Fuer einen neuen Lauf --reset angeben."
            );
        }
        clear_target(&target).await?;
    }
    drop(target);

    let client = introspect::connect(pg).await?;
    let catalog = introspect::read_catalog(&client).await?;
    let model = map::build_model(&catalog)?;

    let source = spec_schema::DdicSchemaSource::new(&model.activated())
        .map_err(|e| anyhow::anyhow!("Schema: {e}"))?;
    let store = PostgresStore::connect(&cli.target_pg, Arc::new(source)).await?;

    let report = import::import_all(&client, &store, &model).await?;
    let total: usize = report.written.iter().map(|(_, n)| n).sum();
    println!("imported {total} rows into {} sets", report.written.len());
    Ok(())
}
