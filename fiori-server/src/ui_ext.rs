//! Generische Erweiterung aus den `ui`-Bloecken der `ddic.json`.
//!
//! * Felder mit `ui.format` (eingebaute Formate, siehe `ui_format.js`) bekommen in der
//!   Liste eine formatierte Spalte neben der Rohspalte, die Rohspalte wird ausgeblendet.
//!   Auf der Object Page erscheint ein Abschnitt "Technische Daten" mit allen formatierten
//!   Feldern.
//! * Entitaeten mit `ui.hidden_fields` blenden diese Felder im Standard aus.
//! * `GET /ext/fiori/Format.js` liefert die Formatter, `GET /ext/fiori/fragment/...` die
//!   Fragmente. Die Fragmente werden aus der DDIC erzeugt, nicht als Dateien abgelegt.
//! * `<ext_dir>/<app>/` wird je App unter `/ext/<app>/` ausgeliefert (Assets pro App).

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::{
    Router,
    extract::Path,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::get,
};
use serde_json::{Value, json};
use spec::data_dictionary::DataDictionary;
use spec::{DisplayFormat, EntityUi, FieldSpec};
use file_repo::FileRepository;

use crate::AppState;
use crate::repo_serve;
use crate::extension::Extension;

const FORMAT_JS: &str = include_str!("ui_format.js");
const COMPONENTS_JS: &str = include_str!("ui_components_controller.js");
/// Controller-Erweiterung fuer alle Komponenten-Verwendungen (Modul `fiori.ext.ComponentsController`).
const COMPONENTS_CONTROLLER: &str = "fiori.ext.ComponentsController";
const XML: &str = "application/xml; charset=utf-8";
const JS: &str = "application/javascript; charset=utf-8";

/// Formatiertes Feld: Rohfeld, Ueberschrift, Format.
#[derive(Clone)]
struct FormattedField {
    field: String,
    label: String,
    format: DisplayFormat,
}

/// Abschnitt der Object Page aus einer Komponenten-Verwendung.
#[derive(Clone)]
struct ComponentSection {
    /// Fragmentname ohne `.fragment.xml` (`Component__<Entitaet>__<Instanz>`).
    fragment: String,
    /// Schluessel des Abschnitts (`Component_<Instanz>`).
    key: String,
    title: String,
    /// Fertiges Fragment-XML.
    xml: String,
}

pub struct UiExtension {
    /// Entitaet (Hauptentitaet) -> Abschnitte aus Komponenten-Verwendungen.
    components: BTreeMap<String, Vec<ComponentSection>>,
    /// Entitaet -> formatierte Felder.
    formatted: BTreeMap<String, Vec<FormattedField>>,
    /// Entitaet -> ausgeblendete Felder.
    hidden: BTreeMap<String, Vec<String>>,
    /// Entitaet -> `ui`-Block (Abschnitte, Spalten, Listen-Tabs, ausgeblendete Facetten).
    entity_ui: BTreeMap<String, EntityUi>,
    /// Hauptentitaet -> App-Name (Namespace der Assets).
    app_of_entity: BTreeMap<String, String>,
    /// Apps mit Assets unter `<ext>/<app>/` (nur vorhandene).
    assets: Vec<String>,
    /// Quelle der Assets (`ext/`).
    ext: Option<Arc<dyn FileRepository>>,
    /// Module je App (`<app>/ext/<modul>`).
    modules: Vec<String>,
    /// Controller-Erweiterungen je App: (Ziel, `<app>.ext.<controller>`).
    controller_exts: Vec<(String, String)>,
}

impl UiExtension {
    /// Liest die `ui`-Bloecke aus dem Dictionary. `ext`: Quelle der App-Assets.
    pub fn new(ddic: &DataDictionary, ext: Option<Arc<dyn FileRepository>>) -> Self {
        let mut formatted: BTreeMap<String, Vec<FormattedField>> = BTreeMap::new();
        let mut hidden: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for entity in ddic.entities() {
            for f in entity.fields.iter() {
                if let FieldSpec::Atom {
                    name,
                    label,
                    ui: Some(ui),
                    ..
                } = f
                {
                    if let Some(format) = ui.format {
                        formatted
                            .entry(entity.name.clone())
                            .or_default()
                            .push(FormattedField {
                                field: name.clone(),
                                label: label.clone(),
                                format,
                            });
                    }
                }
            }
            if let Some(ui) = &entity.ui {
                if !ui.hidden_fields.is_empty() {
                    hidden.insert(entity.name.clone(), ui.hidden_fields.clone());
                }
            }
        }
        let mut entity_ui = BTreeMap::new();
        for entity in ddic.entities() {
            if let Some(ui) = &entity.ui {
                entity_ui.insert(entity.name.clone(), ui.clone());
            }
        }
        let mut app_of_entity = BTreeMap::new();
        let mut assets = Vec::new();
        let mut modules = Vec::new();
        let mut controller_exts = Vec::new();
        for app in ddic.apps() {
            let ns = app.name().to_string();
            app_of_entity.insert(app.main_entity(ddic).name().to_string(), ns.clone());
            let has_assets = ext
                .as_ref()
                .is_some_and(|r| r.list(&ns).map(|v| !v.is_empty()).unwrap_or(false));
            if has_assets {
                assets.push(ns.clone());
            }
            if let Some(app_ui) = app.ui() {
                for m in &app_ui.modules {
                    modules.push(format!("{ns}/ext/{m}"));
                }
                for c in &app_ui.controller_extensions {
                    controller_exts.push((c.target.clone(), format!("{ns}.ext.{}", c.controller)));
                }
            }
        }
        let mut components: BTreeMap<String, Vec<ComponentSection>> = BTreeMap::new();
        for usage in ddic.component_uses() {
            let Some(section_title) = usage.section_title.clone() else {
                continue;
            };
            let Some(component) = ddic.components().iter().find(|c| c.name == usage.component)
            else {
                continue;
            };
            let fragment = format!("Component__{}__{}", usage.entity, usage.instance);
            components
                .entry(usage.entity.clone())
                .or_default()
                .push(ComponentSection {
                    key: format!("Component_{}", usage.instance),
                    title: section_title,
                    xml: component_fragment(component, usage),
                    fragment,
                });
        }
        Self {
            components,
            formatted,
            hidden,
            entity_ui,
            app_of_entity,
            assets,
            ext,
            modules,
            controller_exts,
        }
    }

    /// Namespace der Fragmente einer Entitaet: `<app>.ext.fragment`.
    fn fragment_ns(&self, entity: &str) -> Option<String> {
        self.app_of_entity
            .get(entity)
            .map(|app| format!("{app}.ext.fragment"))
    }
}

/// Fragmentname fuer ein formatiertes Feld: `<field>__<format>`.
fn field_fragment_name(field: &str, format: DisplayFormat) -> String {
    format!("{}__{}", field, format_key(format))
}

/// Fragmentname fuer den Object-Page-Abschnitt einer Entitaet.
fn section_fragment_name(entity: &str) -> String {
    format!("Formatted__{}", entity)
}

fn format_key(format: DisplayFormat) -> &'static str {
    match format {
        DisplayFormat::Duration => "duration",
        DisplayFormat::Bytes => "bytes",
        DisplayFormat::Bitrate => "bitrate",
        DisplayFormat::Date => "date",
        DisplayFormat::Datetime => "datetime",
    }
}

/// Nur Buchstaben, Ziffern und `_`: Namen landen im XML und im Pfad.
fn is_safe_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Fragment einer Komponenten-Verwendung: Beginn/Ende als Text (Anzeige) bzw. Auswahl
/// (Bearbeiten), Dauer als Text bzw. Eingabefeld. Konfiguration fuer den Controller im
/// customData `timeRange`. Feldnamen sind geprueft (nur Buchstaben, Ziffern, `_`).
fn component_fragment(
    component: &spec::component::ComponentSpec,
    usage: &spec::component::ComponentUse,
) -> String {
    use spec::component::{default_label, effective_step};
    let step = effective_step(component, usage);
    let kind = component.kind;
    let field = |role: &str| usage.fields.get(role).cloned().unwrap_or_default();
    let label = |role: &str| {
        xml_escape(
            usage
                .labels
                .get(role)
                .map(String::as_str)
                .unwrap_or(default_label(kind, role)),
        )
    };
    let config = format!(
        "instance={};start={};end={};duration={};step={}",
        usage.instance,
        field("start"),
        field("end"),
        field("duration"),
        step
    );
    let dt = |f: &str| {
        format!(
            "{{path: '{f}', type: 'sap.ui.model.odata.type.DateTimeOffset', constraints: {{V4: true, precision: 7}}, formatOptions: {{pattern: 'dd.MM.yyyy HH:mm'}}}}"
        )
    };
    format!(
        r#"<core:FragmentDefinition xmlns:core="sap.ui.core" xmlns="sap.m" xmlns:f="sap.ui.layout.form">
            <f:Form editable="true">
            <f:customData>
                <core:CustomData key="timeRange" value="{config}"/>
            </f:customData>
            <f:layout>
                <f:ResponsiveGridLayout labelSpanL="3" labelSpanM="3" columnsL="1" columnsM="1"/>
            </f:layout>
            <f:formContainers>
                <f:FormContainer>
                    <f:formElements>
                        <f:FormElement label="{start_label}">
                            <f:fields>
                                <Text visible="{{= !${{ui>/isEditable}} }}" text="{dt}"/>
                                <DateTimePicker visible="{{ui>/isEditable}}" value="{dt}" displayFormat="dd.MM.yyyy HH:mm" minutesStep="{step}"/>
                            </f:fields>
                        </f:FormElement>
                        <f:FormElement label="{end_label}">
                            <f:fields>
                                <Text visible="{{= !${{ui>/isEditable}} }}" text="{dt_end}"/>
                                <DateTimePicker visible="{{ui>/isEditable}}" value="{dt_end}" displayFormat="dd.MM.yyyy HH:mm" minutesStep=""/></f:fields></f:FormElement><f:FormElement label="{duration_label}"><f:fields><Text visible="{{= !${{ui>/isEditable}} }}" text="{{path: '{dur}', targetType: 'any', formatter: 'fiori.ext.Format.minutes'}}"/><Input visible="{{ui>/isEditable}}" placeholder="z. B. 2:30 oder 2h oder 30 min" value="{{path: '{dur}', mode: 'OneWay', targetType: 'any', formatter: 'fiori.ext.Format.hm'}}"/></f:fields></f:FormElement></f:formElements></f:FormContainer></f:formContainers></f:Form></core:FragmentDefinition>"#,
        config = xml_escape(&config),
        start_label = label("start"),
        end_label = label("end"),
        duration_label = label("duration"),
        dt = dt(&field("start")),
        dt_end = dt(&field("end")),
        dur = field("duration"),
        step = step,
    )
}

/// Fragment fuer ein einzelnes formatiertes Feld (Text-Bindung mit Formatter).
fn field_fragment(field: &str, format: DisplayFormat) -> String {
    format!(
        r#"<core:FragmentDefinition xmlns:core="sap.ui.core" xmlns="sap.m"><Text text="{{path: '{field}', targetType: 'any', formatter: 'fiori.ext.Format.{fmt}'}}"/></core:FragmentDefinition>"#,
        field = field,
        fmt = format_key(format),
    )
}

/// Abschnitt "Technische Daten": SimpleForm mit allen formatierten Feldern.
fn section_fragment(fields: &[FormattedField]) -> String {
    let mut content = String::new();
    for f in fields {
        content.push_str(&format!(
            r#"<Label text="{label}"/><Text text="{{path: '{field}', targetType: 'any', formatter: 'fiori.ext.Format.{fmt}'}}"/>"#,
            label = xml_escape(&f.label),
            field = f.field,
            fmt = format_key(f.format),
        ));
    }
    format!(
        r#"<core:FragmentDefinition xmlns:core="sap.ui.core" xmlns="sap.m" xmlns:f="sap.ui.layout.form"><f:SimpleForm editable="false" layout="ResponsiveGridLayout" columnsL="3" columnsM="2"><f:content>{content}</f:content></f:SimpleForm></core:FragmentDefinition>"#
    )
}

impl Extension for UiExtension {
    fn routes(&self, _state: AppState) -> Router {
        let formatted = self.formatted.clone();
        let components = self.components.clone();
        let fragments = move |Path(file): Path<String>| {
            let formatted = formatted.clone();
            let components = components.clone();
            async move {
                let name = match file.strip_suffix(".fragment.xml") {
                    Some(n) if is_safe_name(n) => n.to_string(),
                    _ => return (StatusCode::NOT_FOUND, "").into_response(),
                };
                if name.starts_with("Component__") {
                    let found = components.values().flatten().find(|c| c.fragment == name);
                    return match found {
                        Some(c) => ([(header::CONTENT_TYPE, XML)], c.xml.clone()).into_response(),
                        None => (StatusCode::NOT_FOUND, "").into_response(),
                    };
                }
                if let Some(entity) = name.strip_prefix("Formatted__") {
                    return match formatted.get(entity) {
                        Some(fields) => ([(header::CONTENT_TYPE, XML)], section_fragment(fields))
                            .into_response(),
                        None => (StatusCode::NOT_FOUND, "").into_response(),
                    };
                }
                // `<field>__<format>`: Feld gegen die DDIC pruefen, nicht blind ausliefern.
                let found = formatted
                    .values()
                    .flatten()
                    .find(|f| field_fragment_name(&f.field, f.format) == name);
                match found {
                    Some(f) => (
                        [(header::CONTENT_TYPE, XML)],
                        field_fragment(&f.field, f.format),
                    )
                        .into_response(),
                    None => (StatusCode::NOT_FOUND, "").into_response(),
                }
            }
        };
        let mut router = Router::new()
            .route(
                "/ext/fiori/Format.js",
                get(|| async { ([(header::CONTENT_TYPE, JS)], FORMAT_JS) }),
            )
            .route(
                "/ext/fiori/ComponentsController.controller.js",
                get(|| async { ([(header::CONTENT_TYPE, JS)], COMPONENTS_JS) }),
            )
            .route("/ext/fiori/fragment/{file}", get(fragments));
        if let Some(repo) = self.ext.clone() {
            let apps = self.assets.clone();
            router = router.route(
                "/ext/{app}/{*rest}",
                get(move |Path((app, rest)): Path<(String, String)>| {
                    let repo = repo.clone();
                    let apps = apps.clone();
                    async move {
                        if !apps.contains(&app) {
                            return StatusCode::NOT_FOUND.into_response();
                        }
                        repo_serve::serve(repo.as_ref(), &format!("{app}/{rest}"))
                    }
                }),
            );
        }
        router
    }

    fn ui5_paths(&self) -> Vec<(String, String)> {
        let mut paths = vec![("fiori.ext".into(), "/ext/fiori".into())];
        for app in &self.assets {
            paths.push((format!("{app}.ext"), format!("/ext/{app}")));
        }
        paths
    }

    fn ui5_modules(&self) -> Vec<String> {
        let mut out = vec!["fiori/ext/Format".into()];
        out.extend(self.modules.iter().cloned());
        out
    }

    fn controller_extensions(&self) -> Vec<(String, String)> {
        let mut out = self.controller_exts.clone();
        let target = "sap.fe.templates.ObjectPage.ObjectPageController";
        if !self.components.is_empty() && !out.iter().any(|(t, _)| t == target) {
            out.push((target.to_string(), COMPONENTS_CONTROLLER.to_string()));
        }
        out
    }

    /// Ausgeblendet im Formular: die Felder aus `ui.hidden_fields` und die Rohfelder der
    /// formatierten Felder (ihre Werte stehen in den formatierten Spalten bzw. im Abschnitt).
    fn hidden_fields(&self, entity: &str) -> Vec<String> {
        let mut out = self.hidden.get(entity).cloned().unwrap_or_default();
        if let Some(fields) = self.formatted.get(entity) {
            out.extend(fields.iter().map(|f| f.field.clone()));
        }
        out
    }

    /// Listenansichten mit eigenem Fragment (`ui.list_templates`).
    fn list_views(&self, entity: &str) -> Vec<Value> {
        let (Some(ns), Some(ui)) = (self.fragment_ns(entity), self.entity_ui.get(entity)) else {
            return Vec::new();
        };
        ui.list_templates
            .iter()
            .map(|t| {
                json!({
                    "key": t.key,
                    "label": t.label,
                    "template": format!("{ns}.{}", t.fragment),
                })
            })
            .collect()
    }

    fn list_columns(&self, entity: &str) -> Option<Value> {
        let formatted = self.formatted.get(entity);
        let custom = self
            .entity_ui
            .get(entity)
            .map(|u| u.list_columns.as_slice())
            .unwrap_or(&[]);
        if formatted.is_none() && custom.is_empty() {
            return None;
        }
        let mut cols = serde_json::Map::new();
        for c in custom {
            let ns = self.fragment_ns(entity).unwrap_or_default();
            cols.insert(
                c.key.clone(),
                json!({
                    "header": c.header,
                    "template": format!("{ns}.{}", c.fragment),
                    "width": c.width.clone().unwrap_or_else(|| "120px".into()),
                    "horizontalAlign": "Begin",
                    "position": {
                        "anchor": c.anchor,
                        "placement": c.placement.clone().unwrap_or_else(|| "After".into())
                    }
                }),
            );
        }
        for f in formatted.into_iter().flatten() {
            let key = format!("Fmt{}", f.field);
            cols.insert(
                key,
                json!({
                    "header": f.label,
                    "template": format!("fiori.ext.fragment.{}", field_fragment_name(&f.field, f.format)),
                    "horizontalAlign": "End",
                    "width": "110px",
                    "position": { "anchor": f.field, "placement": "After" }
                }),
            );
            cols.insert(
                format!("DataField::{}", f.field),
                json!({ "visible": false }),
            );
        }
        Some(Value::Object(cols))
    }

    fn object_page_content(&self, entity: &str) -> Option<Value> {
        let mut sections = serde_json::Map::new();
        if self.formatted.contains_key(entity) {
            sections.insert(
                "FormattedFields".into(),
                json!({
                    "title": "Technische Daten",
                    "template": format!("fiori.ext.fragment.{}", section_fragment_name(entity)),
                    "position": { "placement": "After", "anchor": "GeneralSection" }
                }),
            );
        }
        if let (Some(ns), Some(ui)) = (self.fragment_ns(entity), self.entity_ui.get(entity)) {
            for sec in &ui.sections {
                sections.insert(
                    sec.key.clone(),
                    json!({
                        "title": sec.title,
                        "template": format!("{ns}.{}", sec.fragment),
                        "position": {
                            "placement": sec.placement.clone().unwrap_or_else(|| "After".into()),
                            "anchor": sec.anchor.clone().unwrap_or_else(|| "GeneralSection".into())
                        }
                    }),
                );
            }
        }
        for sec in self.components.get(entity).into_iter().flatten() {
            sections.insert(
                sec.key.clone(),
                json!({
                    "title": sec.title,
                    "template": format!("fiori.ext.fragment.{}", sec.fragment),
                    "position": { "placement": "After", "anchor": "GeneralSection" }
                }),
            );
        }
        if let Some(ui) = self.entity_ui.get(entity) {
            for facet in &ui.hide_facets {
                sections.insert(facet.clone(), json!({ "visible": false }));
            }
        }
        if sections.is_empty() {
            None
        } else {
            Some(json!({ "body": { "sections": sections } }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ddic_with_video() -> DataDictionary {
        let video: spec::EntitySpec = serde_json::from_value(json!({
            "name": "Video",
            "package": "video",
            "type_name_plural": "Videos",
            "fields": [
                {"Atom": {"name": "title", "label": "Titel", "edm_type": "Edm.String"}},
                {"Atom": {"name": "duration_seconds", "label": "Dauer", "edm_type": "Edm.Int32",
                          "ui": {"format": "duration"}}},
                {"Atom": {"name": "file_size", "label": "Groesse", "edm_type": "Edm.Int64",
                          "ui": {"format": "bytes"}}},
                {"Atom": {"name": "secret", "label": "Geheim", "edm_type": "Edm.String"}}
            ],
            "ui": {"hidden_fields": ["secret"]}
        }))
        .unwrap();
        let mut ddic = DataDictionary::empty();
        ddic.add_entity(video);
        ddic
    }

    #[test]
    fn formatted_fields_become_list_columns_and_hide_raw_columns() {
        let ext = UiExtension::new(&ddic_with_video(), None);
        let cols = ext.list_columns("Video").expect("Spalten");
        assert_eq!(
            cols["Fmtduration_seconds"]["template"],
            "fiori.ext.fragment.duration_seconds__duration"
        );
        assert_eq!(
            cols["Fmtfile_size"]["template"],
            "fiori.ext.fragment.file_size__bytes"
        );
        assert_eq!(cols["DataField::duration_seconds"]["visible"], false);
        assert!(ext.list_columns("Other").is_none());
    }

    #[test]
    fn object_page_section_lists_formatted_fields() {
        let ext = UiExtension::new(&ddic_with_video(), None);
        let page = ext.object_page_content("Video").expect("Abschnitt");
        assert_eq!(
            page["body"]["sections"]["FormattedFields"]["template"],
            "fiori.ext.fragment.Formatted__Video"
        );
        let fields = ext.formatted.get("Video").unwrap();
        let xml = section_fragment(fields);
        assert!(xml.contains("formatter: 'fiori.ext.Format.duration'"));
        assert!(xml.contains("formatter: 'fiori.ext.Format.bytes'"));
        assert!(xml.contains("<Label text=\"Dauer\"/>"));
    }

    #[test]
    fn hidden_fields_come_from_entity_ui_block() {
        let ext = UiExtension::new(&ddic_with_video(), None);
        let hidden = ext.hidden_fields("Video");
        assert!(hidden.contains(&"secret".to_string()));
        assert!(hidden.contains(&"duration_seconds".to_string()));
        assert!(hidden.contains(&"file_size".to_string()));
        assert!(!hidden.contains(&"title".to_string()));
        assert!(ext.hidden_fields("Other").is_empty());
    }


    #[test]
    fn field_fragment_is_built_from_ddic_only() {
        let ext = UiExtension::new(&ddic_with_video(), None);
        let all: Vec<&FormattedField> = ext.formatted.values().flatten().collect();
        let hit = all
            .iter()
            .find(|f| field_fragment_name(&f.field, f.format) == "file_size__bytes");
        assert!(hit.is_some());
        assert!(!is_safe_name("../etc/passwd"));
        assert!(is_safe_name("file_size__bytes"));
    }
}
