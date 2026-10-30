//! Aufbau und Start des HTTP-Servers.
//!
//! Beide Binaries laufen durch diese Funktion. Was sie unterscheidet, ist
//! ausschliesslich die Herkunft von Dictionary und Schema:
//!
//! | | Designer (`--designer`) | Laufzeit |
//! |---|---|---|
//! | Dictionary | eincompiliertes Metamodell | `--ddic` |
//! | Store | `DdicStore` auf `ddic.json` | SQLite / PostgreSQL |
//! | Schema | - | `--schema`, sonst abgeleitet |

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::{Router, routing::get};
use data_store::{DraftDataStore, Schema, SchemaSource, StaticSchema};
use postgres_relational_store::PostgresStore;
use spec::data_dictionary::DataDictionary;
use spec_schema::DdicSchemaSource;
use sqlite_relational_store::RelationalStore;
use tower_http::{services::ServeDir, trace::TraceLayer};
use tracing::info;

use crate::config::{ServerConfig, ServerMode};
use crate::ddic_store::DdicStore;
use crate::hook::Extender;
use crate::{AppState, batch, flp, srv};

/// Server ohne Erweiterungen starten.
pub async fn run(config: ServerConfig) -> anyhow::Result<()> {
    run_with(config, Vec::new()).await
}

/// Server mit Erweiterungen (siehe `extension::Extension`) aufbauen und bis zum Abbruch bedienen.
pub async fn run_with(
    config: ServerConfig,
    mut extensions: crate::extension::Extensions,
) -> anyhow::Result<()> {
    let config = Arc::new(config);
    crate::status::hub().set_config(crate::status::config_json(&config));
    info!(
        "starting {} server{}",
        config.mode.label(),
        if config.designer { " (designer)" } else { "" }
    );

    // Skripte zuletzt registriert: ihre Hooks liegen aussen und sehen jeden Request zuerst.
    if let Some(dir) = &config.scripts_dir {
        let hooks = script_hooks::ScriptHooks::load(dir)
            .with_context(|| format!("cannot load scripts '{}'", dir.display()))?;
        extensions.push(Arc::new(crate::script::ScriptExtension::new(hooks)));
    }

    let base = load_dictionary(&config)?;
    // Generische UI-Erweiterung aus den `ui`-Bloecken der ddic.json (Formate, ausgeblendete
    // Felder, App-Assets). Wird nach den Skripten eingetragen, damit sie nicht ueberschrieben werden.
    extensions.push(Arc::new(crate::ui_ext::UiExtension::new(&base, config.ext_dir.clone())));
    let (inner, sqlite) = if config.designer {
        // Designer: Metamodell als Basis, Daten aus `ddic.json`.
        let path = config.ddic.clone().context("designer needs --ddic")?;
        let store = DdicStore::open(&path)
            .with_context(|| format!("cannot open designer file '{}'", path.display()))?;
        info!("designer on '{}' (metamodel, file store)", path.display());
        (Arc::new(store) as Arc<dyn DraftDataStore>, None)
    } else {
        let schema_source = schema_source(&config, &base)?;
        open_store(&config, schema_source).await?
    };
    // Benutzerverwaltung: Default User sicherstellen und den Akteur-Hook
    // innerhalb aller anderen Hooks legen (siehe `system_user`).
    let inner = if config.designer {
        inner
    } else {
        crate::system_user::ensure_default_user(inner.as_ref())
            .await
            .context("cannot prepare default user")?;
        let audited = base
            .entities()
            .iter()
            .filter(|e| e.is_audited())
            .map(|e| e.setname())
            .collect();
        Extender::new(
            inner,
            Arc::new(crate::system_user::SystemUserHook::new(audited)),
        )?
    };
    let store = wrap_store(inner, &extensions)?;

    if !config.designer {
        seed_value_lists(store.as_ref(), &base).await?;
    }
    let state = AppState::new(store, config.clone(), base).with_extensions(extensions);
    state.reload();
    crate::term3270::spawn(&config, &state);

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

/// Festwerte aus `ddic.json` in die Listen-Entitaeten schreiben (je Liste ein Set
/// `{Liste}Set`). IDs sind deterministisch (`value_list::entry_id`), Verweise aus
/// Fremdschluesseln bleiben also ueber Neustarts gueltig. Vorhandene Eintraege
/// werden aktualisiert statt geloescht; geloescht wird nur, was nicht mehr in der
/// Liste steht.
async fn seed_value_lists(store: &dyn DraftDataStore, ddic: &DataDictionary) -> anyhow::Result<()> {
    use data_store::EntityKey;
    use odata::query::ODataQuery;
    use serde_json::json;
    use spec::value_list::{CODE_FIELD, LABEL_FIELD, SORT_FIELD, entry_id};
    use std::collections::{HashMap, HashSet};

    if ddic.value_lists().is_empty() {
        return Ok(());
    }
    ddic.validate_value_lists()
        .map_err(|e| anyhow::anyhow!("value lists: {e}"))?;
    let mut count = 0;
    for list in ddic.value_lists() {
        let set = format!("{}Set", list.name);
        let existing = store
            .get_collection(&set, &ODataQuery::new())
            .await
            .map_err(|e| anyhow::anyhow!("cannot read {set}: {e}"))?;
        let wanted: HashSet<uuid::Uuid> = list
            .values
            .iter()
            .map(|v| entry_id(&list.id, &v.code))
            .collect();
        // Alle Saetze je ID (aktiv und Entwurf).
        let mut by_id: HashMap<uuid::Uuid, Vec<_>> = HashMap::new();
        for r in existing {
            by_id.entry(r.id()).or_default().push(r);
        }
        for (id, rows) in &by_id {
            if wanted.contains(id) {
                continue;
            }
            for r in rows {
                store
                    .delete_entity(&set, &EntityKey::new(r.id(), r.is_active_entity()))
                    .await
                    .map_err(|e| anyhow::anyhow!("cannot remove stale entry in {set}: {e}"))?;
            }
        }
        for v in &list.values {
            let id = entry_id(&list.id, &v.code);
            let rows = by_id.get(&id).map(Vec::as_slice).unwrap_or(&[]);
            let active = rows.iter().find(|r| r.is_active_entity());
            let wanted_data = json!({LABEL_FIELD: v.label, CODE_FIELD: v.code, SORT_FIELD: v.sort});
            match active {
                Some(record) => {
                    let unchanged = record.get(LABEL_FIELD) == Some(json!(v.label))
                        && record.get(CODE_FIELD) == Some(json!(v.code))
                        && record.get(SORT_FIELD) == Some(json!(v.sort));
                    if !unchanged {
                        store
                            .patch_entity(&set, &EntityKey::new(id, true), &wanted_data)
                            .await
                            .map_err(|e| anyhow::anyhow!("cannot update {set}: {e}"))?;
                    }
                    // Entwuerfe aus frueheren Starts verwerfen.
                    for r in rows.iter().filter(|r| !r.is_active_entity()) {
                        store
                            .delete_entity(&set, &EntityKey::new(r.id(), false))
                            .await
                            .map_err(|e| anyhow::anyhow!("cannot remove draft in {set}: {e}"))?;
                    }
                }
                None => {
                    let mut data = wanted_data;
                    data["ID"] = json!(id.to_string());
                    let created = store
                        .create_entity(&set, &data)
                        .await
                        .map_err(|e| anyhow::anyhow!("cannot seed {set}: {e}"))?;
                    store
                        .draft_activate(&set, &EntityKey::new(created.id(), false))
                        .await
                        .map_err(|e| anyhow::anyhow!("cannot activate {set}: {e}"))?;
                }
            }
            count += 1;
        }
    }
    info!("{count} value list entries loaded");
    Ok(())
}

/// Ausgangs-Dictionary bestimmen.
///
/// Der Designer kennt sein Modell selbst. Die Laufzeit hat ohne Exportdatei
/// nichts zu tun und bricht deshalb mit einem Hinweis ab, statt mit einem
/// leeren Launchpad zu starten.
fn load_dictionary(config: &ServerConfig) -> anyhow::Result<DataDictionary> {
    if config.designer {
        // Der Designer zeigt sein eincompiliertes Metamodell; die Daten liegen im Store.
        return Ok(DataDictionary::new());
    }
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
            let mut ddic: DataDictionary = serde_json::from_reader(std::io::BufReader::new(file))
                .with_context(|| {
                format!("'{}' is not a valid data dictionary", path.display())
            })?;
            // Benutzerverwaltung direkt nach dem Einlesen (nur im Speicher).
            ddic.inject_system_user();
            let mut ddic = match &config.ddic_overlay {
                Some(path) => {
                    info!("applying ddic overlay {}", path.display());
                    let overlay = crate::overlay::load(path)?;
                    crate::overlay::apply(ddic, &overlay)
                        .with_context(|| format!("ddic overlay '{}'", path.display()))?
                }
                None => ddic,
            };
            // Festwertlisten: Listen-Entitaeten erzeugen und Atome im Speicher zu Lookups
            // machen (vor dem Schema, damit die Spalten dort stehen).
            ddic.apply_value_list_lookups().map_err(|e| {
                anyhow::anyhow!("'{}': ungueltige Festwertliste: {e}", path.display())
            })?;
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
    info!(
        "store at '{}' (in-memory, persisted on shutdown)",
        config.data_dir.display()
    );
    let store = Arc::new(store);
    Ok((store.clone(), Some(store)))
}

/// Haengt die Erweiterungs-Hooks um den Store.
fn wrap_store(
    store: Arc<dyn DraftDataStore>,
    extensions: &crate::extension::Extensions,
) -> anyhow::Result<Arc<dyn DraftDataStore>> {
    let mut store = store;
    // Erweiterungs-Hooks: der zuletzt umhuellende Hook sieht den Request zuerst.
    // Damit `before` in Registrierungsreihenfolge laeuft, wird rueckwaerts gewickelt.
    let hooks: Vec<_> = extensions.iter().flat_map(|ext| ext.hooks()).collect();
    for hook in hooks.into_iter().rev() {
        store = Extender::new(store, hook)?;
    }
    Ok(store)
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
        .route("/", get(flp::index))
        .merge(crate::status::router())
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
        .layer(axum::middleware::from_fn(crate::status::count_requests))
        .layer(TraceLayer::new_for_http())
}
