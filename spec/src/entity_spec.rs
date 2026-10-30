use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// How a measure field relates to its unit/currency companion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MeasureKind {
    /// `Org.OData.Measures.V1.ISOCurrency Path="{unit_field}"`
    Currency,
    /// `Org.OData.Measures.V1.Unit Path="{unit_field}"`
    Unit,
}

/// A context filter for value lists: "my local field X must match target field Y".
/// Generates a `Common.ValueListParameterIn` in the OData annotation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValueListFilter {
    /// Property on the local entity (e.g. "ConfigID", "ID")
    pub local_property: String,
    /// Property on the value list target entity (e.g. "ConfigID")
    pub target_property: String,
}

/// Value help configuration for an Atom field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AtomValueList {
    /// Code/Description from a FieldValueList (identified by UUID).
    /// Generates: Common.ValueList → FieldValueListItems with ListID filter.
    FieldValueList {
        /// UUID of the FieldValueList
        list_id: String,
        /// `false` = dropdown (Common.ValueListWithFixedValues), `true` = search dialog
        prefer_dialog: bool,
    },
    /// Reference to another EntitySet as value help (e.g. "Customers").
    /// Generates: Common.ValueList → target EntitySet with ID as key.
    EntityRef {
        /// Target EntitySet name
        entity_set: String,
        /// Key property in target (default: "ID")
        key_property: String,
        /// Display property in target (e.g. "CustomerName")
        display_property: Option<String>,
        /// Context filters — each generates a `Common.ValueListParameterIn`.
        /// Empty = unfiltered lookup.
        filters: Vec<ValueListFilter>,
        /// `false` = dropdown, `true` = search dialog
        prefer_dialog: bool,
    },
}

impl AtomValueList {
    /// Unfiltered entity reference value help.
    pub fn entity_ref(
        entity_set: &str,
        key_property: &str,
        display_property: Option<&str>,
    ) -> Self {
        AtomValueList::EntityRef {
            entity_set: entity_set.into(),
            key_property: key_property.into(),
            display_property: display_property.map(Into::into),
            filters: vec![],
            prefer_dialog: false,
        }
    }

    /// Pick from siblings under the same parent (entities sharing a common parent FK).
    ///
    /// Example: EntityField picks FormGroup from EntityFacets sharing the same ConfigID.
    /// Filter: `(my ConfigID) = (sibling's ConfigID)` — shows only siblings under the same parent.
    pub fn from_siblings(
        sibling_entity_set: &str,
        shared_fk: &str,
        key_property: &str,
        display_property: Option<&str>,
    ) -> Self {
        AtomValueList::EntityRef {
            entity_set: sibling_entity_set.into(),
            key_property: key_property.into(),
            display_property: display_property.map(Into::into),
            filters: vec![ValueListFilter {
                local_property: shared_fk.into(),
                target_property: shared_fk.into(),
            }],
            prefer_dialog: false,
        }
    }

    /// Set prefer_dialog (builder style).
    pub fn dialog(mut self) -> Self {
        match &mut self {
            AtomValueList::FieldValueList { prefer_dialog, .. } => *prefer_dialog = true,
            AtomValueList::EntityRef { prefer_dialog, .. } => *prefer_dialog = true,
        }
        self
    }
}

/// Suffix appended to a field name when a synthetic single-value / key alias is needed
/// (e.g. a many-to-one relationship's local key field `Order_ID`). One authoritative
/// source so no caller hard-codes the string.
pub const ID_SUFFIX: &str = "_ID";

/// Hilfsfunktion fuer `skip_serializing_if`: Default-`false` wird nicht geschrieben.
pub(crate) fn is_false(value: &bool) -> bool {
    !*value
}

/// Eingebaute Anzeigeformate fuer Atom-Felder. Die Formatierung uebernimmt das
/// Format-Modul des Servers (`fiori-server/src/ui_ext.rs`), nicht die Annotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayFormat {
    /// Sekunden als `h:mm:ss` bzw. `m:ss`.
    Duration,
    /// Bytes als `KB`, `MB`, ... (1024er Schritte).
    Bytes,
    /// Bitrate: `kbit/s` bzw. `Mbit/s`.
    Bitrate,
    /// Datum nach Benutzer-Locale.
    Date,
    /// Datum und Uhrzeit nach Benutzer-Locale.
    Datetime,
}

/// `ui`-Block eines Atom-Feldes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct FieldUi {
    /// Eingebautes Format. Fehlt es, wird der Rohwert angezeigt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<DisplayFormat>,
}

/// `ui`-Block einer Entitaet. Fragmente liegen als `<app>.ext.fragment.<fragment>`
/// im Asset-Verzeichnis der App (`<ext_dir>/<app>/fragment/<fragment>.fragment.xml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EntityUi {
    /// Felder, die im Standard ausgeblendet werden (Listen- und Formularspalten).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_fields: Vec<String>,
    /// Zusaetzliche Abschnitte der Object Page (Custom Sections).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<UiSection>,
    /// Object-Page-Facetten (Ids wie `_VideoThumbnailsSection`), die ausgeblendet werden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hide_facets: Vec<String>,
    /// Zusaetzliche Listenspalten (Custom Columns).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_columns: Vec<UiListColumn>,
    /// Zusaetzliche Listenansichten mit eigenem Fragment (z. B. eine Suchseite als Tab).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_templates: Vec<UiListTemplate>,
}

/// Custom Section der Object Page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSection {
    /// Schluessel des Abschnitts (wird als Id verwendet).
    pub key: String,
    /// Titel; kann ein i18n-Verweis `{{key}}` sein.
    pub title: String,
    /// Fragmentname ohne `.fragment.xml`.
    pub fragment: String,
    /// Anker (Abschnitts-Id), Standard `GeneralSection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<String>,
    /// `After` (Standard) oder `Before`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
}

/// Custom Column der Liste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiListColumn {
    pub key: String,
    pub header: String,
    pub fragment: String,
    /// Anker-Spalte (Feldname), vor oder nach der sie erscheint.
    pub anchor: String,
    /// `After` (Standard) oder `Before`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<String>,
}

/// Listenansicht mit eigenem Fragment (Tab der ListReport-Seite).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiListTemplate {
    pub key: String,
    pub label: String,
    pub fragment: String,
}

/// A field specification — describes the entity's own data (leaves on the relationship tree).
///
/// Only two variants: Atom (simple values) and Measure (value + unit/currency dependency).
/// References to other entities are expressed as `Relationship`, not as fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldSpec {
    Lookup {
        name: String,
        entity: String,
        collection: bool,
        owned: bool,
        /// Lesbarer Titel der Navigation (aus der Relationship). Fehlt er, gilt der Name.
        label: Option<String>,
    },
    /// Simple value: text, number, date, boolean, coded value with optional dropdown.
    Atom {
        name: String,
        label: String,
        edm_type: String,
        /// Logical package grouping (e.g. "itil", "asset_management").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        package: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_length: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        precision: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<u32>,
        /// Server-generated field — never editable (Core.Computed + NonInsertable).
        #[serde(default, skip_serializing_if = "is_false")]
        computed: bool,
        /// Set at creation, read-only afterward (Core.Immutable).
        #[serde(default, skip_serializing_if = "is_false")]
        immutable: bool,
        /// Optional value help (dropdown/dialog).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value_list: Option<AtomValueList>,
        /// Wert ist eine URL: wird als anklickbarer Link dargestellt (`UI.DataFieldWithUrl`).
        #[serde(default, skip_serializing_if = "is_false")]
        link: bool,
        /// Langtext: mehrzeilige Eingabe (`UI.MultiLineText`). `max_length` begrenzt die Laenge.
        #[serde(default, skip_serializing_if = "is_false")]
        multiline: bool,
        /// Zielfelder, die nach einer Aenderung dieses Feldes nachgeladen werden (`Common.SideEffects`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        side_effects: Vec<String>,
        /// Anzeige-Einstellungen (eingebaute Formate). Unbekannte Schluessel sind ein Fehler.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ui: Option<FieldUi>,
    },
    /// Amount/quantity with a unit/currency companion field.
    /// Generates `Org.OData.Measures.V1.ISOCurrency` or `.Unit` annotation.
    Measure {
        name: String,
        label: String,
        /// Logical package grouping (e.g. "itil", "asset_management").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        package: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        precision: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scale: Option<u32>,
        /// Name of the companion Atom field holding the unit/currency code (e.g. "Currency").
        unit_field: String,
        /// Whether this is a currency amount or a physical quantity.
        kind: MeasureKind,
    },
}
impl std::fmt::Display for FieldSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}
impl FieldSpec {
    /// Return this field's name with the standard ID suffix appended
    /// (`Order` → `Order_ID`). Use instead of `format!("{name}_ID")`.
    pub fn with_id_suffix(&self) -> String {
        format!("{}{}", self.name(), ID_SUFFIX)
    }

    /// Whether this field's name already ends with the standard ID suffix.
    /// Use instead of `f.name().ends_with("_ID")` / `"_ID"`.
    pub fn ends_with_id_suffix(&self) -> bool {
        self.name().ends_with(ID_SUFFIX)
    }

    // ── Constructors ─────────────────────────────────────────────────

    pub fn atom(name: &str, label: &str, edm_type: &str) -> Self {
        FieldSpec::Atom {
            name: name.into(),
            label: label.into(),
            edm_type: edm_type.into(),
            package: None,
            max_length: None,
            precision: None,
            scale: None,
            computed: false,
            immutable: false,
            value_list: None,
            link: false,
            multiline: false,
            side_effects: Vec::new(),
            ui: None,
        }
    }

    pub fn string(name: &str, label: &str, max_length: u32) -> Self {
        FieldSpec::Atom {
            name: name.into(),
            label: label.into(),
            edm_type: "Edm.String".into(),
            package: None,
            max_length: Some(max_length),
            precision: None,
            scale: None,
            computed: false,
            immutable: false,
            value_list: None,
            link: false,
            multiline: false,
            side_effects: Vec::new(),
            ui: None,
        }
    }

    pub fn int(name: &str, label: &str) -> Self {
        FieldSpec::atom(name, label, "Edm.Int32")
    }

    pub fn bool_field(name: &str, label: &str) -> Self {
        FieldSpec::atom(name, label, "Edm.Boolean")
    }

    pub fn decimal(name: &str, label: &str, precision: u32, scale: u32) -> Self {
        FieldSpec::Atom {
            name: name.into(),
            label: label.into(),
            edm_type: "Edm.Decimal".into(),
            package: None,
            max_length: None,
            precision: Some(precision),
            scale: Some(scale),
            computed: false,
            immutable: false,
            value_list: None,
            link: false,
            multiline: false,
            side_effects: Vec::new(),
            ui: None,
        }
    }

    pub fn measure(name: &str, label: &str, unit_field: &str, kind: MeasureKind) -> Self {
        FieldSpec::Measure {
            name: name.into(),
            label: label.into(),
            package: None,
            precision: None,
            scale: None,
            unit_field: unit_field.into(),
            kind,
        }
    }

    // ── Chainable modifiers ──────────────────────────────────────────

    pub fn computed(mut self) -> Self {
        if let FieldSpec::Atom { computed, .. } = &mut self {
            *computed = true;
        }
        self
    }

    pub fn immutable(mut self) -> Self {
        if let FieldSpec::Atom { immutable, .. } = &mut self {
            *immutable = true;
        }
        self
    }

    pub fn pkg(mut self, p: &str) -> Self {
        match &mut self {
            FieldSpec::Atom { package, .. } => *package = Some(p.into()),
            FieldSpec::Measure { package, .. } => *package = Some(p.into()),
            _ => todo!(),
        }
        self
    }

    pub fn with_value_list(mut self, vl: AtomValueList) -> Self {
        if let FieldSpec::Atom { value_list, .. } = &mut self {
            *value_list = Some(vl);
        }
        self
    }

    // ── Accessors ────────────────────────────────────────────────────

    /// Returns the field name regardless of variant.
    pub fn name(&self) -> &str {
        match self {
            FieldSpec::Atom { name, .. } => name,
            FieldSpec::Measure { name, .. } => name,
            FieldSpec::Lookup { name, .. } => name,
        }
    }

    /// Returns the field label regardless of variant.
    pub fn label(&self) -> &str {
        match self {
            FieldSpec::Atom { label, .. } => label,
            FieldSpec::Measure { label, .. } => label,
            FieldSpec::Lookup { name, label, .. } => label.as_deref().unwrap_or(name),
        }
    }

    /// Returns the package assignment regardless of variant.
    pub fn package(&self) -> Option<&str> {
        match self {
            FieldSpec::Atom { package, .. } => package.as_deref(),
            FieldSpec::Measure { package, .. } => package.as_deref(),
            _ => todo!(),
        }
    }

    /// Returns the EDM type regardless of variant.
    pub fn edm_type(&self) -> &str {
        match self {
            FieldSpec::Atom { edm_type, .. } => edm_type,
            FieldSpec::Measure { .. } => "Edm.Decimal",
            FieldSpec::Lookup { .. } => "Edm.Guid",
        }
    }
}

/// Application-level entity specification — describes an entity's own fields and header layout.
///
/// Entities may be introduced by `Relationship` declarations without an explicit `EntitySpec`.
/// In that case they get default fields (ID + Name). An `EntitySpec` refines the entity
/// with additional fields, header info, and data points.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySpec {
    /// Entity name (e.g. "Orders"). Must match the entity name used in Relationships.
    pub name: String,

    /// Logical package grouping (e.g. "itil", "asset_management").
    pub package: String,

    /// Singular display name for the Object Page header (e.g. "Customer").
    /// Frei definierbar, unabhaengig vom Entitaetsnamen. Fehlt er, gilt der Name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name: Option<String>,

    /// Plural display name (e.g. "Orders"). Default: set_name.
    pub type_name_plural: String,

    /// The entity's own fields (leaves). FK fields from Relationships are auto-generated
    /// and should NOT be declared here.
    pub fields: Vec<FieldSpec>,

    /// Benannte Listenansichten (Spaltenauswahl). Leer = Standard (nur `name`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list_views: Vec<ListView>,

    /// Vorbelegte Filterwerte je Atom-Feld (`Common.FilterDefaultValue`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub filter_defaults: BTreeMap<String, FilterDefault>,

    /// Felder der Filterleiste in dieser Reihenfolge (`UI.SelectionFields`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selection_fields: Vec<String>,

    /// Einstellungen je Object-Page-Facette (Tabellenabschnitt einer to-many Navigation).
    /// Fehlt ein Eintrag, gelten die Standardwerte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub facets: Vec<FacetSpec>,

    /// Auditierung (Owner, CreatedBy/At, ChangedBy/At). Standard: `true`.
    /// `false` fuer Entitaeten wie Festwerte, die keinen Owner brauchen.
    #[serde(default = "default_audited", skip_serializing_if = "is_true")]
    pub audited: bool,

    /// Anzeige-Einstellungen der Entitaet (`ui`-Block in `ddic.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<EntityUi>,
}

fn default_audited() -> bool {
    true
}

fn is_true(v: &bool) -> bool {
    *v
}

/// Einstellungen fuer eine Object-Page-Facette, identifiziert ueber `facet`.
/// Weitere Einstellungen kommen als zusaetzliche Felder hinzu.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FacetSpec {
    /// Identifikator der Facette: eine to-many Navigation des Eigentuemers
    /// (z. B. `_ActorSceneMap`), `General` (Feldabschnitt Main), `Default`
    /// (nicht genannte Felder) oder ein frei gewaehlter Schluessel mit `fields`.
    pub facet: String,
    /// Listenansicht der Zielentitaet. Fehlt sie oder ist `default`, gilt das Standard-LineItem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<String>,
    /// Titel des Facetten-Abschnitts. Fehlt er, wird der Schluessel verwendet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Felder des Abschnitts in Anzeigereihenfolge (Atome oder einwertige Navigationen).
    /// Bei `General` ohne Angabe: alle Felder. Bei `Default`: nicht genannte Felder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
}

/// Konstanter Standardwert fuer einen Filter. Zahlen und Datumswerte sind
/// bewusst nicht enthalten (eigener Literal-Typ noetig).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterDefault {
    Text(String),
    Flag(bool),
}

/// Eine benannte Listenansicht einer Entitaet.
///
/// `name = "default"` ist die Standardansicht (`UI.LineItem` ohne Qualifier).
/// Jede andere Ansicht erzeugt `UI.LineItem#<name>` und `UI.PresentationVariant#<name>`.
/// `role` ist vorbereitet fuer eine rollenbasierte Auswahl; heute ohne Wirkung.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListView {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Geordnete Spalten. Eintraege sind Feldnamen: Atome (`file_size`) oder
    /// einwertige Navigationen (`agent`, dargestellt als `agent/name`).
    pub columns: Vec<String>,
}

impl ListView {
    pub const DEFAULT: &'static str = "default";

    pub fn new(name: impl Into<String>, columns: &[&str]) -> Self {
        Self {
            name: name.into(),
            role: None,
            columns: columns.iter().map(|c| c.to_string()).collect(),
        }
    }

    pub fn is_default(&self) -> bool {
        self.name == Self::DEFAULT
    }
}

/// An explicit facet section for the Object Page.
#[derive(Debug, Clone)]
pub struct FacetSectionSpec {
    pub label: String,
    pub id: String,
    pub field_group_qualifier: String,
    pub field_group_label: String,
}

/// An explicit table facet (child collection table on the Object Page).
#[derive(Debug, Clone)]
pub struct TableFacetSpec {
    pub label: String,
    pub id: String,
    pub navigation_property: String,
}

impl EntitySpec {
    pub fn new(
        name: impl Into<String>,
        package: impl Into<String>,
        type_name_plural: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            package: package.into(),
            type_name: None,
            type_name_plural: type_name_plural.into(),
            fields: vec![],
            list_views: vec![],
            filter_defaults: BTreeMap::new(),
            selection_fields: vec![],
            facets: vec![],
            audited: true,
            ui: None,
        }
    }

    /// Schaltet die Auditierung dieser Entitaet ab (kein Owner, keine Audit-Felder).
    pub fn without_audit(mut self) -> Self {
        self.audited = false;
        self
    }

    pub fn is_audited(&self) -> bool {
        self.audited
    }

    pub fn facets(&self) -> &[FacetSpec] {
        &self.facets
    }

    pub fn facet(&self, key: &str) -> Option<&FacetSpec> {
        self.facets.iter().find(|f| f.facet == key)
    }

    pub fn selection_fields(&self) -> &[String] {
        &self.selection_fields
    }

    pub fn filter_defaults(&self) -> &BTreeMap<String, FilterDefault> {
        &self.filter_defaults
    }

    pub fn list_view(mut self, view: ListView) -> Self {
        self.list_views.push(view);
        self
    }

    pub fn list_views(&self) -> &[ListView] {
        &self.list_views
    }

    pub fn field(mut self, fld: FieldSpec) -> Self {
        debug!("add field {}", fld.name());
        self.fields.push(fld);
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn fields(&self) -> impl Iterator<Item = &FieldSpec> {
        self.fields.iter()
    }

    /// Singularer Anzeigename: `type_name`, sonst der Entitaetsname.
    pub fn type_name(&self) -> &str {
        self.type_name.as_deref().unwrap_or(&self.name)
    }

    pub fn setname(&self) -> String {
        format!("{}Set", self.name)
    }
}

#[cfg(test)]
mod ui_block_tests {
    use super::*;

    #[test]
    fn atom_accepts_known_format() {
        let json = r#"{"Atom":{"name":"size","label":"Size","edm_type":"Edm.Int64","ui":{"format":"bytes"}}}"#;
        let f: FieldSpec = serde_json::from_str(json).expect("gueltiger ui-Block");
        match f {
            FieldSpec::Atom { ui, .. } => assert_eq!(ui.unwrap().format, Some(DisplayFormat::Bytes)),
            _ => panic!("Atom erwartet"),
        }
    }

    #[test]
    fn unknown_key_in_field_ui_is_an_error() {
        let json = r#"{"Atom":{"name":"size","label":"Size","edm_type":"Edm.Int64","ui":{"formt":"bytes"}}}"#;
        assert!(serde_json::from_str::<FieldSpec>(json).is_err());
    }

    #[test]
    fn unknown_format_is_an_error() {
        let json = r#"{"Atom":{"name":"size","label":"Size","edm_type":"Edm.Int64","ui":{"format":"furlongs"}}}"#;
        assert!(serde_json::from_str::<FieldSpec>(json).is_err());
    }

    #[test]
    fn unknown_key_in_entity_ui_is_an_error() {
        let ui: Result<EntityUi, _> = serde_json::from_str(r#"{"hidden_field":["x"]}"#);
        assert!(ui.is_err());
        let ok: EntityUi = serde_json::from_str(r#"{"hidden_fields":["x"]}"#).unwrap();
        assert_eq!(ok.hidden_fields, vec!["x".to_string()]);
    }
}
