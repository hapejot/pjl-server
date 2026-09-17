use std::sync::{Arc, RwLock};

use data_store::DraftDataStore;
use spec::data_dictionary::DataDictionary;

pub mod batch;
pub mod config;
pub mod ddic_loader;
pub mod export;
pub mod flp;
pub mod hook;
pub mod server;
pub mod srv;

use config::{ServerConfig, ServerMode};

/// Gemeinsamer Zustand beider Server.
///
/// Der Unterschied zwischen Designer und Laufzeit steckt allein in `base`:
/// der Designer bekommt das eincompilierte Metamodell, die Laufzeit das vom
/// Designer exportierte Dictionary. Alles darunter - OData, `$metadata`, FLP -
/// ist identischer Code.
#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn DraftDataStore>,
    pub config: Arc<ServerConfig>,
    /// Unaktivierter Ausgangsstand; jeder `reload()` geht wieder von hier aus,
    /// damit `activate()` die Lookups nicht mehrfach anhaengt.
    base: Arc<DataDictionary>,
    ddic: Arc<RwLock<Arc<DataDictionary>>>,
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
            base,
            ddic: Arc::new(RwLock::new(Arc::new(active))),
        }
    }

    pub fn mode(&self) -> ServerMode {
        self.config.mode
    }

    /// Designer-Zustand mit Standardkonfiguration und dem eincompilierten
    /// Metamodell - der Einstieg fuer Tests und Beispiele.
    pub fn designer(store: Arc<dyn DraftDataStore>) -> Self {
        Self::new(
            store,
            Arc::new(ServerConfig::defaults(ServerMode::Designer)),
            DataDictionary::new(),
        )
    }

    /// Snapshot des aktuellen Dictionary - bleibt fuer die Dauer eines Requests stabil.
    pub fn ddic(&self) -> Arc<DataDictionary> {
        self.ddic.read().expect("ddic lock").clone()
    }

    /// Dictionary komplett neu aufbauen; Lookups entstehen dabei erneut aus den Relations.
    pub fn reload(&self) -> Arc<DataDictionary> {
        let mut ddic = (*self.base).clone();
        ddic.activate();
        self.store
            .set_navigations(ddic_loader::navigations_from_ddic(&ddic));

        let ddic = Arc::new(ddic);
        *self.ddic.write().expect("ddic lock") = ddic.clone();
        ddic
    }

    /// Der im Store gepflegte Modellstand, so wie ihn der Export sieht.
    ///
    /// Nur im Designer sinnvoll: dort liegen in `EntitySet`, `FieldSet`,
    /// `RelationSet`, `PackageSet` und `AppSet` die vom Anwender definierten
    /// Objekte. Bewusst getrennt von [`AppState::reload`] - der Designer soll
    /// in seiner eigenen Oberflaeche nur das Metamodell zeigen, nicht die
    /// Objekte, die er gerade modelliert.
    pub async fn designed_dictionary(&self) -> DataDictionary {
        let mut ddic = DataDictionary::new();
        ddic_loader::load_ddic_from_store(&mut ddic, self.store.as_ref()).await;
        ddic
    }
}
