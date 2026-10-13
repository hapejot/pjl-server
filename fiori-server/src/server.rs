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
use postgres_relational_store::PostgresStore;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::info;

use crate::config::{ServerConfig, ServerMode};
use crate::hook::{EntityDefaultsHook, Extender};
use crate::{batch, flp, srv, AppState};

/// Server ohne Erweiterungen starten.
pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    run_with(config, Vec::new()).await
}

/// Server mit Erweiterungen (siehe `extension::Extension`) aufbauen und bis zum Abbruch bedienen.
pub async fn run_with(
    config: ServerConfig,
    extensions: crate::extension::Extensions,
) -> anyhow::Result<()> {
    let config = Arc::new(config);
    info!("starting {} server", config.mode.label());

    let base = load_dictionary(&config)?;
    let schema_source = schema_source(&config, &base)?;
    let (inner, sqlite) = open_store(&config, schema_source).await?;
    let store = wrap_store(&config, inner)?;

    let state = AppState::new(store, config.clone(), base).with_extensions(extensions);
    state.reload();

    let app = router(state, &config);

    let addr = SocketAddr::from((config.bind, config.port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("cannot bind {addr}"))?;
    info!("{} listening on http://{addr}", config.mode.label());
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    // Der SQLite-Store laeuft nur im Speicher. Nach dem Stopp der Anfragen
    // wird der Stand auf die Platte geschrieben, bevor der Prozess endet.
    // Postgres ist dauerhaft und braucht diesen Schritt nicht.
    if let Some(sqlite) = sqlite {
        sqlite
            .persist()
            .context("cannot persist store on shutdown")?;
        info!("store persisted to '{}'", config.data_dir.display());
    }
    Ok(())
}

/// Wartet auf SIGINT (Ctrl+C) oder SIGTERM (z. B. `docker stop`).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("shutdown: SIGINT"),
        _ = terminate => info!("shutdown: SIGTERM"),
    }
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
            ddic.validate_list_views().map_err(|e| {
                anyhow::anyhow!("'{}': ungueltige Listenansicht: {e}", path.display())
            })?;
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

/// Oeffnet den Store. Ist `STORE_PG_URL` gesetzt, laeuft er in PostgreSQL;
/// sonst im SQLite-Speicher unter `DATA_DIR`.
///
/// Der zweite Rueckgabewert ist nur beim SQLite-Store gesetzt, weil nur der
/// den Stand beim Beenden auf die Platte schreiben muss.
async fn open_store(
    config: &ServerConfig,
    schema: Arc<dyn SchemaSource>,
) -> anyhow::Result<(Arc<dyn DraftDataStore>, Option<Arc<RelationalStore>>)> {
    if let Some(url) = &config.store_pg_url {
        let store = PostgresStore::connect(url, schema)
            .await
            .context("cannot open postgres store (STORE_PG_URL)")?;
        info!("store in PostgreSQL (persistent, no local file)");
        return Ok((Arc::new(store), None));
    }
    std::fs::create_dir_all(&config.data_dir)
        .with_context(|| format!("cannot create data dir '{}'", config.data_dir.display()))?;
    let store = RelationalStore::open(config.data_dir.clone(), schema)
        .await
        .with_context(|| format!("cannot open store '{}'", config.data_dir.display()))?;
    info!("store at '{}' (in-memory, persisted on shutdown)", config.data_dir.display());
    let store = Arc::new(store);
    Ok((store.clone(), Some(store)))
}

/// Haengt die modusabhaengigen Wrapper um den Store.
fn wrap_store(
    config: &ServerConfig,
    store: Arc<dyn DraftDataStore>,
) -> anyhow::Result<Arc<dyn DraftDataStore>> {
    match config.mode {
        // Nur beim Modellieren sinnvoll: eine neue Entitaet bekommt sofort
        // die Felder `name` und `title`. In der Laufzeit waere das ein
        // Eingriff in fachliche Daten.
        ServerMode::Designer => Ok(Extender::new(store, Arc::new(EntityDefaultsHook))?),
        ServerMode::Runtime => Ok(store),
    }
}

fn router(state: AppState, config: &ServerConfig) -> Router {
    let identity_config = std::sync::Arc::new(config.clone());
    let mut app = Router::new()
        .nest("/Service/", batch::router(srv::router(), state.clone()))
        .nest("/flp/", flp::router())
        .route("/apps/{app}/Component.js", get(flp::app_component_handler))
        .route("/apps/{app}/manifest.json", get(flp::app_manifest_handler))
        .route(
            "/apps/{app}/Component-preload.js",
            get(flp::component_preload_handler),
        )
        .route("/apps/{app}/ext/Publish.js", get(flp::publish_ext_handler))
        .route("/apps/{app}/ext/Format.js", get(flp::format_ext_handler))
        .route("/apps/{app}/ext/fragments/{name}", get(flp::format_fragment_handler))
        .route("/", get(flp::index))
        .route("/sap/bc/lrep/flex/settings", get(flp::lrep_settings))
        .route("/sap/bc/lrep/flex/data/{*rest}", get(flp::lrep_flex_data))
        .with_state(state.clone());
    for ext in state.extensions.iter() {
        app = app.merge(ext.routes(state.clone()));
    }
    app.fallback_service(ServeDir::new(&config.web_root))
        .layer(axum::middleware::from_fn_with_state(
            identity_config,
            crate::auth::identity,
        ))
        .layer(TraceLayer::new_for_http())
}
