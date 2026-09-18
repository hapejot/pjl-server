//! Aufbau und Start des HTTP-Servers.
//!
//! Beide Binaries laufen durch diese Funktion. Was sie unterscheidet, ist
//! ausschliesslich die Herkunft von Dictionary und Schema:
//!
//! | | Designer | Laufzeit |
//! |---|---|---|
//! | Dictionary | eincompiliertes Metamodell | `--ddic` (vom Designer exportiert) |
//! | Schema | daraus abgeleitet | `--schema`, sonst daraus abgeleitet |
//! | Store-Hook | legt Standardfelder an | keiner |
//! | Export | `POST /Service/export` | - |

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::{routing::get, Router};
use data_store::{DraftDataStore, Schema, SchemaSource, StaticSchema};
use spec::data_dictionary::DataDictionary;
use spec_schema::DdicSchemaSource;
use sqlite_relational_store::RelationalStore;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::info;

use crate::config::{ServerConfig, ServerMode};
use crate::hook::{EntityDefaultsHook, Extender};
use crate::{batch, flp, srv, AppState};

/// Server nach Konfiguration aufbauen und bis zum Abbruch bedienen.
pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    let config = Arc::new(config);
    info!("starting {} server", config.mode.label());

    let base = load_dictionary(&config)?;
    let schema_source = schema_source(&config, &base)?;
    let store = open_store(&config, schema_source).await?;

    let state = AppState::new(store, config.clone(), base);
    state.reload();

    let app = router(state, &config);

    let addr = SocketAddr::from((config.bind, config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot bind {addr}"))?;
    info!("{} listening on http://{addr}", config.mode.label());
    axum::serve(listener, app).await?;
    Ok(())
}

/// Ausgangs-Dictionary bestimmen.
///
/// Der Designer kennt sein Modell selbst. Die Laufzeit hat ohne Exportdatei
/// nichts zu tun und bricht deshalb mit einem Hinweis ab, statt mit einem
/// leeren Launchpad zu starten.
fn load_dictionary(config: &ServerConfig) -> anyhow::Result<DataDictionary> {
    match (&config.ddic, config.mode) {
        (Some(path), _) => {
            let file = std::fs::File::open(path).with_context(|| {
                format!(
                    "cannot read data dictionary '{}'. Run the designer and press \
                     'Publish Dictionary' to create it.",
                    path.display()
                )
            })?;
            info!("loading ddic from {}", path.display());
            let ddic: DataDictionary = serde_json::from_reader(std::io::BufReader::new(file))
                .with_context(|| format!("'{}' is not a valid data dictionary", path.display()))?;
            info!(
                "dictionary '{}': {} entities, {} apps",
                path.display(),
                ddic.entities().len(),
                ddic.apps().count()
            );
            Ok(ddic)
        }
        (None, ServerMode::Designer) => Ok(DataDictionary::new()),
        (None, ServerMode::Runtime) => {
            anyhow::bail!("the runtime server needs --ddic (or EXPORT_DIR/ddic.json)")
        }
    }
}

/// Schema entweder aus der Datei nehmen oder aus dem Dictionary ableiten.
///
/// Das Ableiten ist der Normalfall und der Grund, warum modellierte
/// Entitaeten in der Laufzeit ueberhaupt Tabellen bekommen: der
/// `RelationalStore` legt beim Start alles an, was im Schema steht und in der
/// Datenbank fehlt.
fn schema_source(
    config: &ServerConfig,
    base: &DataDictionary,
) -> anyhow::Result<Arc<dyn SchemaSource>> {
    if let Some(path) = &config.schema {
        let schema = Schema::load(path)
            .with_context(|| format!("cannot read schema '{}'", path.display()))?;
        info!(
            "schema '{}': {} sets",
            path.display(),
            schema.sets().count()
        );
        return Ok(Arc::new(StaticSchema::new(schema)));
    }
    let mut activated = base.clone();
    activated.activate();
    let source = DdicSchemaSource::new(&activated)
        .context("the data dictionary does not yield a valid storage schema")?;
    info!("schema derived from the dictionary");
    Ok(Arc::new(source))
}

async fn open_store(
    config: &ServerConfig,
    schema: Arc<dyn SchemaSource>,
) -> anyhow::Result<Arc<dyn DraftDataStore>> {
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("cannot create data dir '{}'", config.data_dir.display()))?;
    let store = Arc::new(
        RelationalStore::open(config.data_dir.clone(), schema)
            .await
            .with_context(|| format!("cannot open store '{}'", config.data_dir.display()))?,
    );
    info!("store at '{}'", config.data_dir.display());

    match config.mode {
        // Nur beim Modellieren sinnvoll: eine neue Entitaet bekommt sofort
        // die Felder `name` und `title`. In der Laufzeit waere das ein
        // Eingriff in fachliche Daten.
        ServerMode::Designer => Ok(Extender::new(store, Arc::new(EntityDefaultsHook))?),
        ServerMode::Runtime => Ok(store),
    }
}

fn router(state: AppState, config: &ServerConfig) -> Router {
    Router::new()
        .nest("/Service/", batch::router(srv::router(), state.clone()))
        .nest("/flp/", flp::router())
        .route("/apps/{app}/Component.js", get(flp::app_component_handler))
        .route("/apps/{app}/manifest.json", get(flp::app_manifest_handler))
        .route("/apps/{app}/ext/Publish.js", get(flp::publish_ext_handler))
        .route("/", get(flp::index))
        .fallback_service(ServeDir::new(&config.web_root))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
