use std::sync::{Arc, RwLock};

use data_store::DraftDataStore;
use spec::data_dictionary::DataDictionary;

pub mod auth;
pub mod batch;
pub mod config;
pub mod ddic_file;
pub mod ddic_store;
pub mod ddic_loader;
pub mod extension;
pub mod flp;
pub mod hook;
pub mod system_user;
pub mod overlay;
pub mod script;
pub mod server;
pub mod srv;
pub mod ui_ext;
pub mod status;
pub mod term3270;

use config::{ServerConfig, ServerMode};

/// Gemeinsamer Zustand beider Server.
///
/// Der Unterschied zwischen Designer und Laufzeit steckt allein in `base` und
/// im Store: der Designer zeigt das eincompilierte Metamodell auf `DdicStore`,
/// die Laufzeit das Dictionary aus `ddic.json`. Alles darunter - OData, `$metadata`, FLP -
/// ist identischer Code.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn DraftDataStore>,
    pub config: Arc<ServerConfig>,
    /// Unaktivierter Ausgangsstand; jeder `reload()` geht wieder von hier aus,
    /// damit `activate()` die Lookups nicht mehrfach anhaengt.
    base: Arc<RwLock<Arc<DataDictionary>>>,
    ddic: Arc<RwLock<Arc<DataDictionary>>>,
    /// Eingereichte Erweiterungen (Routen, UI5-Pfade, Object-Page-Beitraege).
    pub extensions: Arc<Vec<Arc<dyn extension::Extension>>>,
}

impl AppState {
    /// Zustand mit explizitem Ausgangs-Dictionary.
    ///
    /// `base` muss *unaktiviert* sein - `activate()` haengt die aus den
    /// Relationships abgeleiteten Lookups an und ist nicht idempotent.
    pub fn new(
        store: Arc<dyn DraftDataStore>,
        config: Arc<ServerConfig>,
        base: DataDictionary,
    ) -> Self {
        let base = Arc::new(base);
        let mut active = (*base).clone();
        active.activate();
        Self {
            store,
            config,
            base: Arc::new(RwLock::new(base)),
            ddic: Arc::new(RwLock::new(Arc::new(active))),
            extensions: Arc::new(Vec::new()),
        }
    }

    /// Erweiterungen setzen; ersetzt die bisherigen.
    pub fn with_extensions(mut self, extensions: extension::Extensions) -> Self {
        self.extensions = Arc::new(extensions);
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
        let mut ddic = (*base).clone();
        ddic.activate();
        self.store
            .set_navigations(ddic_loader::navigations_from_ddic(&ddic));

        let ddic = Arc::new(ddic);
        *self.ddic.write().expect("ddic lock") = ddic.clone();
        ddic
    }
}
