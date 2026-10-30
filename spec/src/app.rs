use crate::{EntitySpec, data_dictionary::DataDictionary, package::Package};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub modules: Vec<String>,
    /// Controller-Erweiterungen: Ziel-Controller und Controller relativ zu `<app>.ext`
    /// (Datei `<controller>.controller.js`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controller_extensions: Vec<ControllerExtension>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControllerExtension {
    pub target: String,
    pub controller: String,
}

impl App {
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

    pub fn ui(&self) -> Option<&AppUi> {
        self.ui.as_ref()
    }
    // getters
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn title(&self) -> &str {
        &self.title
    }

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

    pub fn subtitle(&self) -> Option<&String> {
        self.subtitle.as_ref()
    }

    pub fn icon(&self) -> Option<&String> {
        self.icon.as_ref()
    }

    pub fn package(&self) -> &Package {
        &self.package
    }
}
