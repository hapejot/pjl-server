use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::value_list::{self, CODE_FIELD, LABEL_FIELD, SORT_FIELD, ValueList};
use crate::{AtomValueList, EntitySpec, FieldSpec, Relationship, app::App, package::Package};
use tracing::*;

/// Suffix der Fremdschluessel-Spalte auf Ziele mit eigenem Primaerschluessel.
pub const CODE_FK_SUFFIX: &str = "_code";

/// EDM-Typen, die als Primaerschluessel eines Atoms zulaessig sind.
pub const PRIMARY_KEY_TYPES: [&str; 4] = ["Edm.String", "Edm.Guid", "Edm.Int32", "Edm.Int64"];

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Das Datenverzeichnis (DDIC): Pakete, Entitäten, Beziehungen, Apps, Komponenten
/// und Festwertlisten.
///
/// Die Serialisierung entspricht dem Format von `ddic.json`.
pub struct DataDictionary {
    packages: Vec<Package>,
    entities: Vec<EntitySpec>,
    relationships: Vec<Relationship>,
    apps: Vec<App>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    value_lists: Vec<ValueList>,
    /// Komponenten-Definitionen (z. B. Zeitraum).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    components: Vec<crate::component::ComponentSpec>,
    /// Verwendungen der Komponenten in Entitaeten. Werden beim Laden expandiert.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    component_uses: Vec<crate::component::ComponentUse>,
    #[serde(skip)]
    active: HashMap<String, EntitySpec>,
    /// Atome mit Festwertliste, die `apply_value_list_lookups` in Lookups umgewandelt hat.
    #[serde(skip)]
    converted_value_fields: Vec<ConvertedValueField>,
}

/// Ein Atom mit Festwertliste, das im Speicher zu einem Lookup wurde.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertedValueField {
    /// Entitaet, zu der das Feld gehoert.
    pub(crate) entity: String,
    /// Feldname (bleibt gleich, die Fremdschluesselspalte heisst `{field}_ID`).
    pub(crate) field: String,
    /// Liste, auf die das Feld zeigt.
    pub(crate) list_id: String,
}

impl ConvertedValueField {
    /// Entitaet, zu der das Feld gehoert.
    pub fn entity(&self) -> &str {
        &self.entity
    }

    /// Feldname.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// Festwertliste, auf die das Feld zeigt.
    pub fn list_id(&self) -> &str {
        &self.list_id
    }
}

impl DataDictionary {
    /// Standard-Datenwoerterbuch mit dem Paket `core` und den eingebauten Entitaeten (z. B. `Entity`).
    /// Fuer ein leeres Dictionary siehe [`DataDictionary::empty`].
    pub fn new() -> Self {
        debug!("using default ddic");
        let _core = Package::new("core");
        let core = Package::new("core");

        let entity_ent = EntitySpec::new("Entity", core.name(), "Entities")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::bool_field("draft_enabled", "Draft Enabled"))
            .field(FieldSpec::atom("plural", "Plural", "Edm.String"))
            .field(FieldSpec::atom("type_name", "Singular Label", "Edm.String"))
            .field(FieldSpec::bool_field("audited", "Audited"));

        let field_ent = EntitySpec::new("Field", core.name(), "Fields")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("role", "Role", "Edm.String"))
            .field(FieldSpec::atom("label", "Label", "Edm.String"))
            .field(FieldSpec::atom("type", "Type", "Edm.String"))
            .field(FieldSpec::atom("max_length", "MaxLength", "Edm.Int64"))
            .field(FieldSpec::atom("precision", "Precision", "Edm.Int64"))
            .field(FieldSpec::atom("scale", "Scale", "Edm.Int64"))
            .field(FieldSpec::atom("computed", "Computed", "Edm.Boolean"))
            .field(FieldSpec::atom("immutable", "Immutable", "Edm.Boolean"))
            .field(FieldSpec::atom("multiline", "Multiline", "Edm.Boolean"))
            .field(FieldSpec::atom("kind", "Field Kind", "Edm.String"))
            .field(FieldSpec::atom("link", "Link", "Edm.Boolean"))
            .field(FieldSpec::bool_field("prefer_dialog", "Prefer Dialog"))
            .field(FieldSpec::atom(
                "value_help_type",
                "Value Help Type",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "entity_set",
                "Target Entity Set",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "key_property",
                "Key Property",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "display_property",
                "Display Property",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "display_format",
                "Display Format",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "unit_field",
                "Unit or Currency Field",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "measure_kind",
                "Measure Kind",
                "Edm.String",
            ));

        let relation_ent = EntitySpec::new("Relation", core.name(), "Relations")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::bool_field("owned", "Owned"))
            .field(FieldSpec::atom(
                "source_field",
                "List Reference",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "target_field",
                "Single Field",
                "Edm.String",
            ))
            .field(FieldSpec::atom("package", "Package", "Edm.String"))
            .field(FieldSpec::atom(
                "one_label",
                "Collection Label",
                "Edm.String",
            ))
            .field(FieldSpec::atom("many_label", "Single Label", "Edm.String"));

        // `name` ist Titel und Listenspalte (Annotationen verweisen darauf); es traegt den Code.
        let value_ent = EntitySpec::new("Value", core.name(), "Values")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("value", "Value", "Edm.String"))
            .field(FieldSpec::atom("label", "Label", "Edm.String"))
            .field(FieldSpec::atom("sort", "Sort", "Edm.Int64"))
            .without_audit();
        let value_list_ent = EntitySpec::new("ValueList", core.name(), "Value Lists")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .without_audit();
        // .field(FieldSpec::atom("package", "Package", "Edm.Guid"));

        let package_ent = EntitySpec::new("Package", core.name(), "Packages")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("title", "Title", "Edm.String"))
            .field(FieldSpec::atom("description", "Description", "Edm.String"));

        let app_ent = EntitySpec::new("App", core.name(), "Apps")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("title", "Title", "Edm.String"))
            .field(FieldSpec::atom("subtitle", "Subtitle", "Edm.String"))
            .field(FieldSpec::atom("icon", "Icon", "Edm.String"));

        let component_ent = EntitySpec::new("Component", core.name(), "Components")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("kind", "Kind", "Edm.String"))
            .field(FieldSpec::atom("script", "Script", "Edm.String"))
            .field(FieldSpec::atom("unit", "Unit", "Edm.String"))
            .field(FieldSpec::atom("step_minutes", "Step Minutes", "Edm.Int64"));

        let component_use_ent = EntitySpec::new("ComponentUse", core.name(), "Component Uses")
            .field(FieldSpec::atom("instance", "Instance", "Edm.String"))
            .field(FieldSpec::atom("step_minutes", "Step Minutes", "Edm.Int64"))
            .field(FieldSpec::atom(
                "section_title",
                "Section Title",
                "Edm.String",
            ));

        let list_view_ent = EntitySpec::new("EntityListView", core.name(), "List Views")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("role", "Role", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let list_column_ent = EntitySpec::new("EntityListColumn", core.name(), "List Columns")
            .field(FieldSpec::atom("field", "Field", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .field(FieldSpec::atom("width", "Width", "Edm.String"))
            .field(FieldSpec::atom("priority", "Priority", "Edm.String"))
            .without_audit();
        let filter_default_ent =
            EntitySpec::new("EntityFilterDefault", core.name(), "Filter Defaults")
                .field(FieldSpec::atom("field", "Field", "Edm.String"))
                .field(FieldSpec::atom("value", "Value", "Edm.String"))
                .field(FieldSpec::bool_field("is_boolean", "Boolean Value"))
                .without_audit();
        let selection_field_ent =
            EntitySpec::new("EntitySelectionField", core.name(), "Selection Fields")
                .field(FieldSpec::atom("field", "Field", "Edm.String"))
                .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
                .without_audit();
        let facet_ent = EntitySpec::new("EntityFacet", core.name(), "Facets")
            .field(FieldSpec::atom("facet", "Facet", "Edm.String"))
            .field(FieldSpec::atom("view", "List View", "Edm.String"))
            .field(FieldSpec::atom("title", "Title", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let facet_field_ent = EntitySpec::new("EntityFacetField", core.name(), "Facet Fields")
            .field(FieldSpec::atom("field", "Field", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let hidden_field_ent = EntitySpec::new("EntityHiddenField", core.name(), "Hidden Fields")
            .field(FieldSpec::atom("field", "Field", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let hidden_facet_ent = EntitySpec::new("EntityHiddenFacet", core.name(), "Hidden Facets")
            .field(FieldSpec::atom("facet", "Facet", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let ui_section_ent = EntitySpec::new("EntityUiSection", core.name(), "Custom Sections")
            .field(FieldSpec::atom("key", "Key", "Edm.String"))
            .field(FieldSpec::atom("title", "Title", "Edm.String"))
            .field(FieldSpec::atom("fragment", "Fragment", "Edm.String"))
            .field(FieldSpec::atom("anchor", "Anchor", "Edm.String"))
            .field(FieldSpec::atom("placement", "Placement", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let ui_column_ent =
            EntitySpec::new("EntityUiListColumn", core.name(), "Custom List Columns")
                .field(FieldSpec::atom("key", "Key", "Edm.String"))
                .field(FieldSpec::atom("header", "Header", "Edm.String"))
                .field(FieldSpec::atom("fragment", "Fragment", "Edm.String"))
                .field(FieldSpec::atom("anchor", "Anchor", "Edm.String"))
                .field(FieldSpec::atom("placement", "Placement", "Edm.String"))
                .field(FieldSpec::atom("width", "Width", "Edm.String"))
                .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
                .without_audit();
        let ui_template_ent =
            EntitySpec::new("EntityUiListTemplate", core.name(), "List Templates")
                .field(FieldSpec::atom("key", "Key", "Edm.String"))
                .field(FieldSpec::atom("label", "Label", "Edm.String"))
                .field(FieldSpec::atom("fragment", "Fragment", "Edm.String"))
                .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
                .without_audit();
        let field_side_effect_ent = EntitySpec::new("FieldSideEffect", core.name(), "Side Effects")
            .field(FieldSpec::atom("target", "Target Field", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let value_help_filter_ent =
            EntitySpec::new("ValueHelpFilter", core.name(), "Value Help Filters")
                .field(FieldSpec::atom(
                    "local_property",
                    "Local Field",
                    "Edm.String",
                ))
                .field(FieldSpec::atom(
                    "target_property",
                    "Target Field",
                    "Edm.String",
                ))
                .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
                .without_audit();
        let app_module_ent = EntitySpec::new("AppModule", core.name(), "App Modules")
            .field(FieldSpec::atom("module", "Module", "Edm.String"))
            .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
            .without_audit();
        let app_extension_ent = EntitySpec::new(
            "AppControllerExtension",
            core.name(),
            "Controller Extensions",
        )
        .field(FieldSpec::atom("target", "Target Controller", "Edm.String"))
        .field(FieldSpec::atom(
            "controller",
            "Extension Controller",
            "Edm.String",
        ))
        .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
        .without_audit();
        let component_role_ent =
            EntitySpec::new("ComponentUseRole", core.name(), "Component Roles")
                .field(FieldSpec::atom("role", "Role", "Edm.String"))
                .field(FieldSpec::atom("field", "Field", "Edm.String"))
                .field(FieldSpec::atom("label", "Label", "Edm.String"))
                .field(FieldSpec::atom("order", "Position", "Edm.Int64"))
                .without_audit();

        let rel = Relationship::new("entity_fields", core.name())
            .with_one("Entity", "_Fields")
            .with_many("Field", "entity")
            .owned();
        let r2 = Relationship::new("entity_package", core.name())
            .with_many("Entity", "package")
            .with_one("Package", "_Entities");
        let r3 = Relationship::new("many_entity", core.name())
            .with_one("Entity", "_Many")
            .with_many("Relation", "many");
        let r4 = Relationship::new("one_entity", core.name())
            .with_one("Entity", "_One")
            .with_many("Relation", "one");
        let r5 = Relationship::new("field_package", core.name())
            .with_many("Field", "package")
            .with_one("Package", "_Fields");
        let r6 = Relationship::new("app_entity", core.name())
            .with_one("Entity", "_Apps")
            .with_many("App", "main_entity");
        let r8 = Relationship::new("value_list_values", core.name())
            .with_one("ValueList", "_Values")
            .with_many("Value", "list");
        let r9 = Relationship::new("value_list_fields", core.name())
            .with_one("ValueList", "_Fields")
            .with_many("Field", "value_list");
        let config_relationships = [
            (
                "entity_list_views",
                "Entity",
                "_ListViews",
                "EntityListView",
                "entity",
            ),
            (
                "list_view_columns",
                "EntityListView",
                "_Columns",
                "EntityListColumn",
                "list_view",
            ),
            (
                "entity_filter_defaults",
                "Entity",
                "_FilterDefaults",
                "EntityFilterDefault",
                "entity",
            ),
            (
                "entity_selection_fields",
                "Entity",
                "_SelectionFields",
                "EntitySelectionField",
                "entity",
            ),
            (
                "entity_facets",
                "Entity",
                "_Facets",
                "EntityFacet",
                "entity",
            ),
            (
                "facet_fields",
                "EntityFacet",
                "_Fields",
                "EntityFacetField",
                "facet",
            ),
            (
                "entity_hidden_fields",
                "Entity",
                "_HiddenFields",
                "EntityHiddenField",
                "entity",
            ),
            (
                "entity_hidden_facets",
                "Entity",
                "_HiddenFacets",
                "EntityHiddenFacet",
                "entity",
            ),
            (
                "entity_ui_sections",
                "Entity",
                "_UiSections",
                "EntityUiSection",
                "entity",
            ),
            (
                "entity_ui_columns",
                "Entity",
                "_UiColumns",
                "EntityUiListColumn",
                "entity",
            ),
            (
                "entity_ui_templates",
                "Entity",
                "_UiTemplates",
                "EntityUiListTemplate",
                "entity",
            ),
            (
                "field_side_effects",
                "Field",
                "_SideEffects",
                "FieldSideEffect",
                "field",
            ),
            (
                "value_help_filters",
                "Field",
                "_ValueHelpFilters",
                "ValueHelpFilter",
                "field",
            ),
            ("app_modules", "App", "_Modules", "AppModule", "app"),
            (
                "app_controller_extensions",
                "App",
                "_ControllerExtensions",
                "AppControllerExtension",
                "app",
            ),
            (
                "entity_component_uses",
                "Entity",
                "_ComponentUses",
                "ComponentUse",
                "entity",
            ),
            (
                "component_uses_component",
                "Component",
                "_Uses",
                "ComponentUse",
                "component",
            ),
            (
                "component_use_roles",
                "ComponentUse",
                "_Roles",
                "ComponentUseRole",
                "component_use",
            ),
        ]
        .into_iter()
        .map(|(name, one, one_field, many, many_property)| {
            Relationship::new(name, core.name())
                .with_one(one, one_field)
                .with_many(many, many_property)
                .owned()
        })
        .collect::<Vec<_>>();
        let r7 = Relationship::new("app_package", core.name())
            .with_one("Package", "_Apps")
            .with_many("App", "package");
        let a1 = App::new(
            "relations".into(),
            "Relations".into(),
            "Relation".into(),
            Some("Maintain Relations".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a2 = App::new(
            "values".into(),
            "Values".into(),
            "Value".into(),
            Some("Maintain Values".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a3 = App::new(
            "entities".into(),
            "Entities".into(),
            "Entity".into(),
            Some("Maintain Entities".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a4 = App::new(
            "packages".into(),
            "Packages".into(),
            "Package".into(),
            Some("Maintain Packages".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a6 = App::new(
            "valuelists".into(),
            "Value Lists".into(),
            "ValueList".into(),
            Some("Maintain Value Lists".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a5 = App::new(
            "apps".into(),
            "Apps".into(),
            "App".into(),
            Some("Maintain Apps".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a7 = App::new(
            "components".into(),
            "Components".into(),
            "Component".into(),
            Some("Maintain Components".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a8 = App::new(
            "componentuses".into(),
            "Component Uses".into(),
            "ComponentUse".into(),
            Some("Maintain Component Uses".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );

        Self {
            packages: vec![core],
            entities: vec![
                entity_ent,
                relation_ent,
                field_ent,
                value_ent,
                package_ent,
                app_ent,
                value_list_ent,
                component_ent,
                component_use_ent,
                list_view_ent,
                list_column_ent,
                filter_default_ent,
                selection_field_ent,
                facet_ent,
                facet_field_ent,
                hidden_field_ent,
                hidden_facet_ent,
                ui_section_ent,
                ui_column_ent,
                ui_template_ent,
                field_side_effect_ent,
                value_help_filter_ent,
                app_module_ent,
                app_extension_ent,
                component_role_ent,
            ],
            relationships: vec![rel, r2, r3, r4, r5, r6, r7, r8, r9]
                .into_iter()
                .chain(config_relationships)
                .collect(),
            apps: vec![a1, a2, a3, a4, a5, a6, a7, a8],
            value_lists: Vec::new(),
            components: Vec::new(),
            component_uses: Vec::new(),
            active: HashMap::new(),
            converted_value_fields: Vec::new(),
        }
    }

    /// Ein leeres Dictionary - Basis fuer gefilterte Kopien (z.B. beim Export,
    /// wo das Metamodell selbst nicht mit in den Laufzeitserver soll).
    ///
    /// `new()` liefert dagegen immer das eincompilierte Metamodell.
    pub fn empty() -> Self {
        Self {
            packages: Vec::new(),
            entities: Vec::new(),
            relationships: Vec::new(),
            apps: Vec::new(),
            value_lists: Vec::new(),
            components: Vec::new(),
            component_uses: Vec::new(),
            active: HashMap::new(),
            converted_value_fields: Vec::new(),
        }
    }

    /// Alle Apps.
    pub fn apps(&self) -> impl Iterator<Item = &App> {
        self.apps.iter()
    }

    /// Alle Pakete.
    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }

    /// Alle Komponenten-Definitionen.
    pub fn components(&self) -> &[crate::component::ComponentSpec] {
        &self.components
    }

    /// Alle Verwendungen von Komponenten in Entitaeten (vor der Expansion).
    pub fn component_uses(&self) -> &[crate::component::ComponentUse] {
        &self.component_uses
    }

    /// Erzeugt aus den Komponenten-Verwendungen Felder, Nebenwirkungen und ausgeblendete
    /// Felder. Muss vor [`DataDictionary::apply_value_list_lookups`] und vor dem Ableiten
    /// des Schemas laufen. Idempotent.
    pub fn expand_components(&mut self) -> Result<(), String> {
        crate::component::expand(&mut self.entities, &self.components, &self.component_uses)
    }

    /// Alle Festwertlisten.
    pub fn value_lists(&self) -> &[ValueList] {
        &self.value_lists
    }

    /// Primaerschluessel pruefen: genau ein Atom des Entitaet, nicht berechnet, ohne
    /// Wertehilfe, mit einem Typ aus [`PRIMARY_KEY_TYPES`]. Ohne Drafts, da sonst
    /// `IsActiveEntity` in den Schluessel gehoeren wuerde.
    pub fn validate_primary_keys(&self) -> Result<(), String> {
        for entity in &self.entities {
            let Some(key) = entity.primary_key.as_deref() else {
                continue;
            };
            let ctx = format!("{}.primary_key '{}'", entity.name(), key);
            if entity.draft_enabled {
                return Err(format!("{ctx}: eigener Schluessel nur ohne Drafts"));
            }
            let field = entity
                .fields()
                .find(|f| f.name() == key)
                .ok_or_else(|| format!("{ctx}: Feld existiert nicht"))?;
            match field {
                FieldSpec::Atom {
                    computed,
                    value_list,
                    edm_type,
                    ..
                } => {
                    if *computed {
                        return Err(format!("{ctx}: berechnetes Feld kann kein Schluessel sein"));
                    }
                    if value_list.is_some() {
                        return Err(format!("{ctx}: Schluessel darf keine Wertehilfe haben"));
                    }
                    if !PRIMARY_KEY_TYPES.contains(&edm_type.as_str()) {
                        return Err(format!(
                            "{ctx}: Typ '{edm_type}' nicht als Schluessel zulaessig"
                        ));
                    }
                }
                _ => return Err(format!("{ctx}: Schluessel muss ein Atom sein")),
            }
        }
        Ok(())
    }

    /// Festwertlisten pruefen (siehe [`value_list::validate`]).
    pub fn validate_value_lists(&self) -> Result<(), String> {
        value_list::validate(&self.value_lists)
    }

    /// Je Festwertliste eine generierte Entitaet, benannt nach der Liste. Ohne
    /// Listen entsteht keine.
    fn generated_entities(&self) -> Vec<EntitySpec> {
        self.value_lists
            .iter()
            .map(|list| {
                let code = if list.code_key {
                    FieldSpec::string(CODE_FIELD, "Wert", 50)
                } else {
                    FieldSpec::atom(CODE_FIELD, "Wert", "Edm.String")
                };
                let entity = EntitySpec::new(&list.name, "core", &list.name)
                    .field(FieldSpec::atom(LABEL_FIELD, "Bezeichnung", "Edm.String"))
                    .field(code)
                    .field(FieldSpec::atom(SORT_FIELD, "Sortierung", "Edm.Int64"))
                    .without_audit();
                if list.code_key {
                    entity.without_drafts().with_primary_key(CODE_FIELD)
                } else {
                    entity
                }
            })
            .collect()
    }

    /// Name der Fremdschluessel-Spalte fuer eine einwertige Navigation `field` auf
    /// `target`. Hat das Ziel einen eigenen Primaerschluessel, heisst sie `{field}_code`
    /// (der Wert ist der Schluessel); sonst `{field}_ID` (GUID).
    ///
    /// Der Name darf nicht dem Feld selbst entsprechen: OData verlangt, dass Navigationen
    /// und Properties eines Typs verschiedene Namen haben.
    pub fn fk_column(&self, target: &str, field: &str) -> String {
        let own_key = self
            .resolve_entity_any(target)
            .is_some_and(|e| e.primary_key.is_some());
        if own_key {
            format!("{field}{CODE_FK_SUFFIX}")
        } else {
            format!("{field}_ID")
        }
    }

    /// Entitaet nach Name, aktiv falls vorhanden, sonst im Entwurfsstand.
    fn resolve_entity_any(&self, name: &str) -> Option<&EntitySpec> {
        self.active
            .get(name)
            .or_else(|| self.entities.iter().find(|e| e.name() == name))
    }

    /// Wandelt Atome mit Festwertliste im Speicher in Lookups auf die Listen-Entitaet
    /// um. `ddic.json` bleibt unveraendert. Die Umwandlung merkt sich die Felder, damit
    /// die Migration der alten Textspalten (Code -> ID) geplant werden kann.
    ///
    /// Muss vor dem Ableiten des Schemas laufen. Idempotent: bereits umgewandelte
    /// Felder sind keine Atome mehr.
    pub fn apply_value_list_lookups(&mut self) -> Result<(), String> {
        self.validate_value_lists()?;
        for list in &self.value_lists {
            if !value_list::is_valid_entity_name(&list.name) {
                return Err(format!(
                    "value list '{}': Name ist kein gueltiger Entitaetsname",
                    list.name
                ));
            }
            if self.entities.iter().any(|e| e.name == list.name) {
                return Err(format!(
                    "value list '{}': Name kollidiert mit einer Entitaet",
                    list.name
                ));
            }
        }

        let lists = self.value_lists.clone();
        let mut converted = Vec::new();
        for entity in self.entities.iter_mut() {
            for field in entity.fields.iter_mut() {
                let FieldSpec::Atom {
                    name,
                    label,
                    value_list: Some(AtomValueList::FieldValueList { list_id, .. }),
                    ..
                } = field
                else {
                    continue;
                };
                let list = lists
                    .iter()
                    .find(|l| l.id.eq_ignore_ascii_case(list_id))
                    .ok_or_else(|| {
                        format!(
                            "{}.{}: unbekannte Festwertliste '{}'",
                            entity.name, name, list_id
                        )
                    })?;
                converted.push(ConvertedValueField {
                    entity: entity.name.clone(),
                    field: name.clone(),
                    list_id: list.id.clone(),
                });
                *field = FieldSpec::Lookup {
                    name: name.clone(),
                    entity: list.name.clone(),
                    collection: false,
                    owned: false,
                    label: Some(label.clone()),
                };
            }
        }
        self.converted_value_fields = converted;
        Ok(())
    }

    /// Felder, die bei `apply_value_list_lookups` von Atom zu Lookup wurden.
    pub fn converted_value_fields(&self) -> &[ConvertedValueField] {
        &self.converted_value_fields
    }

    /// Ergaenzt das Dictionary um die Benutzerverwaltung: Entitaet `SystemUser`,
    /// Audit-Felder (`Owner`, `CreatedBy`, `ChangedBy`, `CreatedAt`, `ChangedAt`)
    /// auf jeder Entitaet und die App fuer `SystemUser`.
    ///
    /// Idempotent. Eine in der Datei deklarierte `SystemUser` wird ergaenzt;
    /// gleichnamige Felder werden durch die Vorgabe ersetzt. Nur im Speicher.
    pub fn inject_system_user(&mut self) {
        use crate::system_user as su;

        self.add_package(Package::new(su::PACKAGE));

        let user_fields = [
            FieldSpec::string(su::USERNAME, "Name", 255),
            FieldSpec::string(su::NAME, "Full Name", 255),
            FieldSpec::bool_field(su::ACTIVE, "Active"),
            FieldSpec::string(su::EMAIL, "E-Mail", 255),
            FieldSpec::string(su::THEME, "Theme", 64),
            FieldSpec::string(su::CONTENT_DENSITY, "Content Density", 16),
            FieldSpec::bool_field(su::ACCESSIBILITY, "Accessibility Mode"),
            FieldSpec::bool_field(su::DETECT_DARK_MODE, "Detect Dark Mode"),
            FieldSpec::string(su::LANGUAGE, "Language", 16),
            FieldSpec::string(su::TIME_ZONE, "Time Zone", 64),
            FieldSpec::string(su::CALENDAR_WEEK_NUMBERING, "Calendar Week Numbering", 32),
            FieldSpec::bool_field(su::TRACK_USER_ACTIVITIES, "Track User Activities"),
            FieldSpec::bool_field(su::SHOW_MY_HOME, "Show My Home"),
            FieldSpec::bool_field(su::IMPORT_BOOKMARKS, "Import Bookmarks"),
            FieldSpec::bool_field(su::IMAGE_CONSENT, "Image Consent"),
            FieldSpec::bool_field(su::TRACK_USAGE_ANALYTICS, "Track Usage Analytics"),
        ];
        match self.entities.iter_mut().find(|e| e.name == su::SYSTEM_USER) {
            Some(entity) => {
                for field in user_fields {
                    replace_field(&mut entity.fields, field);
                }
            }
            None => {
                let mut entity = EntitySpec::new(su::SYSTEM_USER, su::PACKAGE, "System Users");
                for field in user_fields {
                    entity = entity.field(field);
                }
                self.entities.push(entity);
            }
        }

        for entity in self.entities.iter_mut().filter(|e| e.is_audited()) {
            for field in su::audit_fields() {
                replace_field(&mut entity.fields, field);
            }
        }

        if !self.apps.iter().any(|a| a.name() == "systemusers") {
            self.apps.push(App::new(
                "systemusers".into(),
                "System Users".into(),
                su::SYSTEM_USER.into(),
                Some("Maintain System Users".into()),
                Some("sap-icon://person-placeholder".into()),
                Package::new(su::PACKAGE),
            ));
        }
    }

    /// Alle Entitaeten inklusive generierter (z. B. aus Festwertlisten), mit den aus
    /// Beziehungen ergaenzten Feldern.
    pub fn entities(&self) -> Vec<EntitySpec> {
        let mut entities = self.entities.clone();
        entities.extend(self.generated_entities());
        for relation in self.relationships.iter() {
            let Relationship {
                one,
                one_field,
                many,
                many_property,
                ..
            } = relation;
            if let Some(e) = entities
                .iter_mut()
                .find(|x| &x.name == one.as_ref().unwrap())
            {
                trace!("adding one {}", relation.name);
                e.fields.push(FieldSpec::Lookup {
                    name: one_field.clone().unwrap(),
                    entity: many.clone().unwrap(),
                    collection: true,
                    owned: false,
                    label: relation.one_label.clone(),
                });
            }

            if let Some(e) = entities
                .iter_mut()
                .find(|x| &x.name == many.as_ref().unwrap())
            {
                trace!("adding many {}", relation.name);
                e.fields.push(FieldSpec::Lookup {
                    name: many_property.clone().unwrap(),
                    entity: one.clone().unwrap(),
                    collection: false,
                    owned: false,
                    label: relation.many_label.clone(),
                });
            }
        }
        entities
    }

    /// Der dateibasierte Designer hat keinen Entwurfszustand.
    pub fn disable_drafts(&mut self) {
        for entity in &mut self.entities {
            entity.draft_enabled = false;
        }
    }

    /// Alle Beziehungen.
    pub fn relationships(&self) -> impl Iterator<Item = &Relationship> {
        self.relationships.iter()
    }

    /// Composition in der `entity` die abhaengige (N-)Seite ist.
    pub fn owning_relationship(&self, entity: &str) -> Option<&Relationship> {
        self.relationships
            .iter()
            .find(|r| r.is_owned() && r.many.as_deref() == Some(entity))
    }

    /// Compositions, deren Kinder an `entity` haengen.
    pub fn owned_children(&self, entity: &str) -> impl Iterator<Item = &Relationship> {
        self.relationships
            .iter()
            .filter(move |r| r.is_owned() && r.one.as_deref() == Some(entity))
    }

    /// Register a package unless one with the same name already exists.
    /// Festwertliste registrieren (ersetzt eine Liste gleicher ID).
    pub fn add_value_list(&mut self, list: ValueList) {
        self.value_lists
            .retain(|l| !l.id.eq_ignore_ascii_case(&list.id));
        self.value_lists.push(list);
    }

    /// Fuegt ein Paket hinzu. Gibt `false` zurueck, wenn der Name bereits existiert.
    pub fn add_package(&mut self, package: Package) -> bool {
        if self.packages.iter().any(|p| p.name() == package.name()) {
            return false;
        }
        self.packages.push(package);
        true
    }

    /// Registriert eine Entitaet, es sei denn, es existiert bereits eine mit dem gleichen Namen.
    pub fn add_entity(&mut self, entity: EntitySpec) -> bool {
        if self.entities.iter().any(|e| e.name() == entity.name()) {
            return false;
        }
        self.entities.push(entity);
        true
    }

    /// Registrier eine Beziehunge, falls keine mit gleichem Namen existiert.
    pub fn add_relationship(&mut self, relationship: Relationship) -> bool {
        if self
            .relationships
            .iter()
            .any(|r| r.name() == relationship.name())
        {
            return false;
        }
        self.relationships.push(relationship);
        true
    }

    /// Register an app unless one with the same name already exists.
    pub fn add_app(&mut self, app: App) -> bool {
        if self.apps.iter().any(|a| a.name() == app.name()) {
            return false;
        }
        self.apps.push(app);
        true
    }

    /// Append a field to an already registered entity.
    ///
    /// Lookups are never added here - they are derived from the relationships.
    pub fn add_field(&mut self, entity: &str, field: FieldSpec) -> bool {
        if matches!(field, FieldSpec::Lookup { .. }) {
            return false;
        }
        match self.entities.iter_mut().find(|e| e.name() == entity) {
            Some(e) => {
                debug!("add field: {}", field.name());
                e.fields.push(field);
                true
            }
            None => false,
        }
    }

    /// Names of all registered entities, including the ones loaded at runtime.
    pub fn entity_names(&self) -> impl Iterator<Item = &str> {
        self.entities.iter().map(|e| e.name())
    }

    /// Resolve the foreign-key field on the target side of a `FieldSpec::Lookup`.
    ///
    /// For a scalar lookup (`collection == false`) this is just the lookup's own
    /// name. For a collection lookup we look up the matching `Relationship` (or
    /// fall back to a reverse scalar lookup on the target entity).
    pub fn foreign_key_field(&self, source: &EntitySpec, lookup: &FieldSpec) -> Option<String> {
        let FieldSpec::Lookup {
            name,
            entity: target,
            collection,
            owned: _,
            label: _,
        } = lookup
        else {
            return None;
        };

        if !collection {
            return Some(name.clone());
        }

        self.relationships()
            .find(|relationship| {
                relationship.one() == source.name()
                    && relationship.many() == target
                    && relationship.one_property() == name
            })
            .map(|relationship| relationship.many_property().to_string())
            .or_else(|| {
                let target_entity = self.resolve_entity(target)?;
                target_entity.fields().find_map(|field| match field {
                    FieldSpec::Lookup {
                        name: rev_name,
                        entity: rev_entity,
                        collection: false,
                        owned,
                        ..
                    } if rev_entity == source.name() => Some(rev_name.clone()),
                    _ => None,
                })
            })
    }

    fn resolve_entity(&self, name: &str) -> Option<EntitySpec> {
        if self.active.is_empty() {
            self.entities().into_iter().find(|e| e.name() == name)
        } else {
            self.active.get(name).cloned()
        }
    }

    /// Aktive Entitaet (nach `activate`) mit dem gegebenen Namen.
    pub fn active_entity(&self, name: &str) -> Option<&EntitySpec> {
        self.active.get(name)
    }

    /// Alle aktiven Entitaeten.
    pub fn active_entities(&self) -> impl Iterator<Item = &EntitySpec> {
        self.active.values()
    }

    /// Aktive Entitaet mit dem gegebenen EntitySet-Namen.
    pub fn active_entity_by_setname(&self, set_name: &str) -> Option<&EntitySpec> {
        self.active
            .values()
            .find(|entity| entity.setname() == set_name)
    }

    /// Baut die aktive Sicht (`active`) auf. Beziehungen werden als Lookup-Felder in die
    /// Entitaeten eingetragen; generierte Entitaeten (z. B. aus Festwertlisten) sind enthalten.
    pub fn activate(&mut self) {
        for entitiy in self.entities.iter().chain(self.generated_entities().iter()) {
            let mut active = entitiy.clone();

            for r in self.relationships() {
                if let Relationship {
                    one: Some(one),
                    one_field: Some(one_field),
                    many: Some(many),
                    many_property: Some(many_property),
                    ..
                } = &r
                {
                    if one == &active.name {
                        if !active.fields.iter().any(|x| x.name() == one_field) {
                            debug!("add one field {}", one_field);
                            active.fields.push(FieldSpec::Lookup {
                                name: one_field.clone(),
                                entity: many.clone(),
                                collection: true,
                                owned: false,
                                label: r.one_label.clone(),
                            });
                        }
                    }
                    if many == &active.name {
                        if !active.fields.iter().any(|x| x.name() == many_property) {
                            debug!("add many field {}", many_property);
                            active.fields.push(FieldSpec::Lookup {
                                name: many_property.clone(),
                                entity: one.clone(),
                                collection: false,
                                owned: false,
                                label: r.many_label.clone(),
                            });
                        }
                    }
                }
            }
            self.active.insert(active.name.clone(), active);
        }
    }
    /// Prueft die Listenansichten aller Entitaeten gegen den aktivierten Stand.
    ///
    /// Regeln: Ansichtsnamen eindeutig pro Entitaet, ohne `#` und Leerzeichen;
    /// mindestens eine Spalte; keine doppelten Spalten; jede Spalte ist ein
    /// Atom oder eine einwertige Navigation, deren Ziel ein Feld `name` hat.
    /// Sammelnavigationen sind als Spalte nicht zulaessig.
    pub fn validate_list_views(&self) -> Result<(), String> {
        info!("validiere ListViews");
        let mut active = self.clone();
        active.activate();

        for entity in &self.entities {
            let mut seen_names: HashSet<&str> = HashSet::new();
            for view in entity.list_views() {
                let ctx = format!("{}.list_views['{}']", entity.name(), view.name);
                info!("validiere ListView {}", ctx);
                if view.name.trim().is_empty()
                    || view.name.contains('#')
                    || view.name.chars().any(char::is_whitespace)
                {
                    return Err(format!("{ctx}: ungueltiger Ansichtsname"));
                }
                if seen_names.contains(&view.name.as_str()) {
                    return Err(format!("{ctx}: Ansichtsname doppelt"));
                }
                seen_names.insert(&view.name);
                if view.columns.is_empty() {
                    return Err(format!("{ctx}: keine Spalten"));
                }

                let mut seen_cols: HashSet<&str> = HashSet::new();
                for column in &view.columns {
                    let col = column.field.as_str();
                    if seen_cols.contains(&col) {
                        return Err(format!("{ctx}: Spalte '{col}' doppelt"));
                    }
                    seen_cols.insert(col);
                    check_list_column(&active, entity.name(), col)
                        .map_err(|e| format!("{ctx}: {e}"))?;
                    if let Some(width) = &column.width {
                        if !is_valid_css_width(width) {
                            return Err(format!(
                                "{ctx}: Breite '{width}' von Spalte '{col}' ungueltig (erwartet z. B. 120px, 15rem, 20% oder auto)"
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Gibt eine Zusammenfassung des Dictionarys auf der Standardausgabe aus (Diagnose).
    pub fn dump(&self) {
        println!("==== Apps ===================================");
        for app in self.apps.iter() {
            println!("{:}", app.name());
        }
        println!("==== Packages ===============================");
        for package in self.packages() {
            println!("{:?}", package.name());
        }
        println!("==== Entities ===============================");
        for (_, entity) in self.active.iter() {
            println!("{}", entity.name);
            for prop in entity.fields.iter() {
                println!("    {}", prop.name());
            }
        }
    }
}

impl Default for DataDictionary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;

/// Ersetzt ein gleichnamiges Feld oder haengt es an.
fn replace_field(fields: &mut Vec<FieldSpec>, field: FieldSpec) {
    fields.retain(|f| f.name() != field.name());
    fields.push(field);
}

/// Prueft eine Listenspalte `col` der Entitaet `entity` (aktivierter Stand).
fn check_list_column(active: &DataDictionary, entity: &str, col: &str) -> Result<(), String> {
    let e = active
        .active_entity(entity)
        .ok_or_else(|| format!("Entitaet '{entity}' nicht aktiv"))?;
    let field = e
        .fields()
        .find(|f| f.name() == col)
        .ok_or_else(|| format!("Feld '{col}' existiert nicht"))?;
    match field {
        FieldSpec::Atom { .. } => Ok(()),
        FieldSpec::Lookup {
            entity: target,
            collection: false,
            ..
        } => {
            let has_name = active
                .active_entity(target)
                .is_some_and(|t| t.fields().any(|f| f.name() == "name"));
            if has_name {
                Ok(())
            } else {
                Err(format!("Ziel '{target}' von '{col}' hat kein Feld 'name'"))
            }
        }
        FieldSpec::Lookup { .. } => Err(format!(
            "'{col}' ist eine Sammelnavigation und kann keine Listenspalte sein"
        )),
        FieldSpec::Measure { .. } => Err(format!(
            "Measure '{col}' wird nicht als Spalte unterstuetzt"
        )),
    }
}

/// Zulaessige Spaltenbreite: `auto` oder eine positive Zahl mit Einheit `px`, `rem`, `em` oder `%`.
pub fn is_valid_css_width(width: &str) -> bool {
    if width == "auto" {
        return true;
    }
    let units = ["px", "rem", "em", "%"];
    units.iter().any(|unit| {
        width.strip_suffix(unit).is_some_and(|num| {
            !num.is_empty()
                && num.chars().all(|c| c.is_ascii_digit() || c == '.')
                && num.chars().any(|c| c.is_ascii_digit())
                && num.matches('.').count() <= 1
        })
    })
}

#[cfg(test)]
mod list_view_tests;

#[cfg(test)]
mod value_list_tests;

#[cfg(test)]
mod primary_key_tests;

#[cfg(test)]
mod code_key_tests;
