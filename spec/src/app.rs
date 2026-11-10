use crate::{EntitySpec, data_dictionary::DataDictionary, package::Package};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Eine App (Fiori-Elements-Einstieg): Kachel, Titel und Hauptentitaet.
pub struct App {
    name: String,
    title: String,
    main_entity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    icon: Option<String>,
    package: Package,
    /// Erweiterungen der App-Oberflaeche (`ui`-Block in `ddic.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ui: Option<AppUi>,
}

/// `ui`-Block einer App: Assets unter `<ext_dir>/<app>/` werden als UI5-Namespace
/// `<app>.ext` eingebunden. `modules` und `controller_extensions` verweisen darauf.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppUi {
    /// Module relativ zu `<app>.ext`, z. B. `TagSearch` (Datei `TagSearch.js`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) modules: Vec<String>,
    /// Controller-Erweiterungen: Ziel-Controller und Controller relativ zu `<app>.ext`
    /// (Datei `<controller>.controller.js`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) controller_extensions: Vec<ControllerExtension>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Erweiterungs-Controller einer App: Ziel-Controller und Controller relativ zu `<app>.ext`.
pub struct ControllerExtension {
    pub(crate) target: String,
    pub(crate) controller: String,
}

impl AppUi {
    /// Module relativ zu `<app>.ext`.
    pub fn modules(&self) -> &[String] {
        &self.modules
    }

    /// Controller-Erweiterungen der App.
    pub fn controller_extensions(&self) -> &[ControllerExtension] {
        &self.controller_extensions
    }
}

impl ControllerExtension {
    /// Ziel-Controller, der erweitert wird.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Erweiterungs-Controller relativ zu `<app>.ext`.
    pub fn controller(&self) -> &str {
        &self.controller
    }
}

impl App {
    /// Erzeugt eine App ohne `ui`-Block.
    pub fn new(
        name: String,
        title: String,
        main_entity: String,
        subtitle: Option<String>,
        icon: Option<String>,
        package: Package,
    ) -> Self {
        Self {
            name,
            title,
            main_entity,
            subtitle,
            icon,
            package,
            ui: None,
        }
    }

    /// Setzt den `ui`-Block der App.
    pub fn with_ui(mut self, ui: AppUi) -> Self {
        self.ui = Some(ui);
        self
    }

    /// `ui`-Block der App, falls vorhanden.
    pub fn ui(&self) -> Option<&AppUi> {
        self.ui.as_ref()
    }
    // getters
    /// Technischer Name der App (Namensraum der Assets).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Anzeigetitel der App.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Start-URL der App, abgeleitet aus der Hauptentitaet.
    pub fn url(&self) -> String {
        format!("./apps/{}/", self.main_entity)
    }

    /// Aktion der Kachel bzw. Listen-Einstieg.
    pub fn action(&self) -> &str {
        "List"
    }

    /// Aktion des direkten Einstiegs auf die Object Page (kein Kachel, `ID` Parameter).
    pub fn detail_action(&self) -> &str {
        "Detail"
    }

    // resolvers
    /// Liefert die Hauptentitaet. Die aktive Sicht hat Vorrang vor der Spezifikation.
    ///
    /// # Panics
    ///
    /// Panics, wenn die Hauptentitaet im Dictionary nicht existiert.
    pub fn main_entity<'a>(&self, _ddic: &'a DataDictionary) -> EntitySpec {
        if let Some(entity) = _ddic.active_entity(&self.main_entity) {
            return entity.clone();
        }

        _ddic
            .entities()
            .iter()
            .find(|e| e.name() == self.main_entity)
            .cloned()
            .expect(&format!("Entity {} not found", self.main_entity))
    }

    /// Untertitel der App, falls gesetzt.
    pub fn subtitle(&self) -> Option<&String> {
        self.subtitle.as_ref()
    }

    /// Icon der App, falls gesetzt.
    pub fn icon(&self) -> Option<&String> {
        self.icon.as_ref()
    }

    /// Paket, zu dem die App gehoert.
    pub fn package(&self) -> &Package {
        &self.package
    }
}
