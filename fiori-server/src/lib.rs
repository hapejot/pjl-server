use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use data_store::DraftDataStore;
use file_repo::{FileRepository, Limits};
use javaproperties::Properties;
use spec::data_dictionary::DataDictionary;

pub mod auth;
pub mod batch;
pub mod config;
pub mod ddic_file;
pub mod ddic_store;
// pub mod ddic_loader;
pub mod extension;
pub mod flp;
pub mod hook;
pub mod overlay;
pub mod repo_serve;
pub mod script;
pub mod server;
pub mod sources;
pub mod srv;
pub mod status;
pub mod sticky;
pub mod system_user;
pub mod term3270;
pub mod ui_ext;

use config::{ServerConfig, ServerMode};
use tracing::info;

/// Gemeinsamer Zustand beider Server.
///
/// Der Unterschied zwischen Designer und Laufzeit steckt allein in `base` und
/// im Store: der Designer zeigt das eincompilierte Metamodell auf `DdicStore`,
/// die Laufzeit das Dictionary aus `ddic.json`. Alles darunter - OData, `$metadata`, FLP -
/// ist identischer Code.
#[derive(Clone)]
pub struct AppState {
    pub sticky: Option<Arc<sticky::StickySessions>>,
    pub store: Arc<dyn DraftDataStore>,
    pub config: Arc<ServerConfig>,
    pub file_repo: Option<Arc<dyn FileRepository>>,
    /// Unaktivierter Ausgangsstand; jeder `reload()` geht wieder von hier aus,
    /// damit `activate()` die Lookups nicht mehrfach anhaengt.
    base: Arc<RwLock<Arc<DataDictionary>>>,
    ddic: Arc<RwLock<Arc<DataDictionary>>>,
    /// Eingereichte Erweiterungen (Routen, UI5-Pfade, Object-Page-Beitraege).
    pub extensions: Arc<Vec<Arc<dyn extension::Extension>>>,
    internationalization: Arc<Properties>,
}

impl AppState {
    /// Zustand mit explizitem Ausgangs-Dictionary.
    ///
    /// `base` muss *unaktiviert* sein - `activate()` haengt die aus den
    /// Relationships abgeleiteten Lookups an und ist nicht idempotent.
    pub fn new(
        store: Arc<dyn DraftDataStore>,
        config: Arc<ServerConfig>,
        mut base: DataDictionary,
    ) -> Self {
        if config.designer {
            base.disable_drafts();
        }
        let base = Arc::new(base);
        let mut active = (*base).clone();
        active.activate();

        let r = config
            .model_root
            .as_ref()
            .map(|root| file_repo::open(&root, &Limits::default()).ok())
            .flatten();

        info!("AppState initialized with active dictionary.");
        info!(?config);

        let mut s = Self {
            sticky: None,
            store,
            config,
            base: Arc::new(RwLock::new(base)),
            ddic: Arc::new(RwLock::new(Arc::new(active))),
            extensions: Arc::new(Vec::new()),
            file_repo: r,
            internationalization: Arc::new(Properties::default()),
        };
        s.init_i18n();
        s
    }

    /// Erweiterungen setzen; ersetzt die bisherigen.
    pub fn with_extensions(mut self, extensions: extension::Extensions) -> Self {
        self.extensions = Arc::new(extensions);
        self
    }

    pub fn with_sticky_sessions(mut self, store: Arc<ddic_store::DdicStore>) -> Self {
        assert!(
            self.is_designer(),
            "Sticky sessions currently require the designer store"
        );
        self.sticky = Some(Arc::new(sticky::StickySessions::new(store)));
        self
    }

    pub fn mode(&self) -> ServerMode {
        self.config.mode
    }

    /// Designer-Modus: Metamodell-Oberflaeche, Schreiben in `ddic.json`.
    pub fn is_designer(&self) -> bool {
        self.config.designer
    }

    /// Designer-Zustand mit Standardkonfiguration und dem eincompilierten
    /// Metamodell - der Einstieg fuer Tests und Beispiele.
    pub fn designer(store: Arc<dyn DraftDataStore>) -> Self {
        let mut config = ServerConfig::defaults(ServerMode::Runtime);
        config.designer = true;
        Self::new(store, Arc::new(config), DataDictionary::new())
    }

    /// Snapshot des aktuellen Dictionary - bleibt fuer die Dauer eines Requests stabil.
    pub fn ddic(&self) -> Arc<DataDictionary> {
        self.ddic.read().expect("ddic lock").clone()
    }

    /// Unaktivierte Basis, so wie sie `reload()` verwendet.
    pub fn base(&self) -> Arc<DataDictionary> {
        self.base.read().expect("base lock").clone()
    }

    /// Ersetzt die Basis nur im Speicher und baut das aktive Dictionary neu auf.
    /// Nichts davon wird im Store oder in einer Datei persistiert.
    pub fn replace_base(&self, base: DataDictionary) -> Arc<DataDictionary> {
        *self.base.write().expect("base lock") = Arc::new(base);
        self.reload()
    }

    /// Dictionary komplett neu aufbauen; Lookups entstehen dabei erneut aus den Relations.
    pub fn reload(&self) -> Arc<DataDictionary> {
        let base = self.base.read().expect("base lock").clone();
        let ddic = (*base).clone();
        // ddic.activate();
        // self.store
        //     .set_navigations(ddic_loader::navigations_from_ddic(&ddic));

        let ddic = Arc::new(ddic);
        // *self.ddic.write().expect("ddic lock") = ddic.clone();
        ddic
    }

    fn init_i18n(&mut self) {
        let mut map = BTreeMap::new();
        for e in self.ddic().entities() {
            for f in e.fields() {
                let key = format!("{}.{}", e.name(), f.name());
                let value = f.label().to_string();
                map.insert(key, value);
            }
            map.insert(format!("{}.name", e.name()), e.type_name().to_string());
        }
        for app in self.ddic().apps() {
            map.insert(format!("app.{}.title", app.name()), app.title().to_string());
            map.insert(
                format!("app.{}.subtitle", app.name()),
                app.subtitle().unwrap_or(&"-".to_string()).to_string(),
            );
        }
        if let Some(content) = self
            .file_repo
            .as_ref()
            .map(|r| r.read("i18n.properties").ok())
            .flatten()
        {
            for (k, v) in javaproperties::parse(&content)
                .ok()
                .unwrap_or_default()
                .iter()
            {
                map.insert(k.to_string(), v.to_string());
            }
        }
        let p2 = Properties::from_pairs(map.into_iter().collect::<Vec<_>>());
        self.internationalization = Arc::new(p2);
    }
}
