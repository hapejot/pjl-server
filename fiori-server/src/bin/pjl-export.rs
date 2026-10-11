use std::sync::Arc;

use fiori_server::{config::ServerConfig, export::export_dictionary, AppState};
use spec::data_dictionary::DataDictionary;
use spec_schema::DdicSchemaSource;
use sqlite_relational_store::RelationalStore;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer().with_file(true).with_line_number(true))
        .with(EnvFilter::from_default_env())
        .init();
    // SETTINGS -------------------------------
    let source_dir = "design-data";
    let target_dir = "sample-data";

    // START ----------------------------------
    info!("exporting schema from `{source_dir}` to `{target_dir}`");

    let mut config = ServerConfig::defaults(fiori_server::config::ServerMode::Designer);
    config.export_dir = target_dir.into();
    let mut ddic = DataDictionary::new();
    ddic.activate();
    // let schema = Schema::load(".")?;
    let source = DdicSchemaSource::new(&ddic)?;
    let store = Arc::new(RelationalStore::open(source_dir, Arc::new(source)).await?);
    let state = AppState::new(store.clone(), Arc::new(config), DataDictionary::new());
    state.reload();
    let report = export_dictionary(&state).await?;
    info!(?report);
    // Betrieb nur im Speicher: Schema-Migration und evtl. Schreibvorgaenge persistieren.
    store.persist()?;
    Ok(())
}
