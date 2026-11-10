use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    pub(crate) local_property: String,
    /// Property on the value list target entity (e.g. "ConfigID")
    pub(crate) target_property: String,
}

/// Fester Filter auf der Value-Help-Entitaet, unabhaengig vom Datensatz.
/// Generates a `Common.ValueListParameterConstant` (z. B. `dimension = "time"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConstantFilter {
    /// Property on the value list target entity (e.g. "dimension")
    pub(crate) target_property: String,
    /// Fester Vergleichswert
    pub(crate) value: String,
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
        /// Feste Filter (z. B. nur Einheiten einer Dimension). Leer = keine Einschraenkung.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        constant_filters: Vec<ConstantFilter>,
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
            constant_filters: vec![],
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
            constant_filters: vec![],
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
    pub(crate) format: Option<DisplayFormat>,
}

/// `ui`-Block einer Entitaet. Fragmente liegen als `<app>.ext.fragment.<fragment>`
/// im Asset-Verzeichnis der App (`<ext_dir>/<app>/fragment/<fragment>.fragment.xml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct EntityUi {
    /// Felder, die im Standard ausgeblendet werden (Listen- und Formularspalten).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) hidden_fields: Vec<String>,
    /// Zusaetzliche Abschnitte der Object Page (Custom Sections).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) sections: Vec<UiSection>,
    /// Object-Page-Facetten (Ids wie `_VideoThumbnailsSection`), die ausgeblendet werden.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) hide_facets: Vec<String>,
    /// Zusaetzliche Listenspalten (Custom Columns).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) list_columns: Vec<UiListColumn>,
    /// Zusaetzliche Listenansichten mit eigenem Fragment (z. B. eine Suchseite als Tab).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) list_templates: Vec<UiListTemplate>,
}

/// Custom Section der Object Page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiSection {
    /// Schluessel des Abschnitts (wird als Id verwendet).
    pub(crate) key: String,
    /// Titel; kann ein i18n-Verweis `{{key}}` sein.
    pub(crate) title: String,
    /// Fragmentname ohne `.fragment.xml`.
    pub(crate) fragment: String,
    /// Anker (Abschnitts-Id), Standard `GeneralSection`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) anchor: Option<String>,
    /// `After` (Standard) oder `Before`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) placement: Option<String>,
}

/// Custom Column der Liste.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiListColumn {
    pub(crate) key: String,
    pub(crate) header: String,
    pub(crate) fragment: String,
    /// Anker-Spalte (Feldname), vor oder nach der sie erscheint.
    pub(crate) anchor: String,
    /// `After` (Standard) oder `Before`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) placement: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) width: Option<String>,
}

/// Listenansicht mit eigenem Fragment (Tab der ListReport-Seite).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UiListTemplate {
    pub(crate) key: String,
    pub(crate) label: String,
    pub(crate) fragment: String,
}

/// A field specification — describes the entity's own data (leaves on the relationship tree).
///
/// Only two variants: Atom (simple values) and Measure (value + unit/currency dependency).
/// References to other entities are expressed as `Relationship`, not as fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FieldSpec {
    /// Verweis auf eine andere Entitaet (Navigation).
    Lookup {
        /// Name des Feldes.
        name: String,
        /// Zielentitaet.
        entity: String,
        /// `true`: to-many Navigation (Sammlung).
        collection: bool,
        /// `true`: Kompositionsbeziehung (Kind gehoert zum Eigentuemer).
        owned: bool,
        /// Lesbarer Titel der Navigation (aus der Relationship). Fehlt er, gilt der Name.
        label: Option<String>,
    },
    /// Simple value: text, number, date, boolean, coded value with optional dropdown.
    Atom {
        /// Atom: skalares Feld mit Typ aus dem EDM.
        name: String,
        /// Name des Feldes.
        label: String,
        /// Anzeigename (Label).
        edm_type: String,
        /// Logical package grouping (e.g. "itil", "asset_management").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        package: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Maximale Laenge (nur Strings).
        max_length: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Genauigkeit (Dezimalzahlen).
        precision: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Nachkommastellen (Dezimalzahlen).
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
        /// Schluessel einer Einheit/Waehrung: Feld der Nachkommastellen im selben Entity
        /// (`Common.UnitSpecificScale`). Nur fuer den Code-Schluessel einer Stammdaten-Entity.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        unit_scale_field: Option<String>,
        /// Code-Liste fuer Einheiten/Waehrungen (`UnitsOfMeasure` oder `CurrencyCodes`).
        /// Die Runtime laedt darueber die Nachkommastellen (`CodeList.v1`-Container-Annotation).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code_list: Option<String>,
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
        /// Dezimalfeld mit Genauigkeit und Nachkommastellen.
        name: String,
        /// Name des Feldes.
        label: String,
        /// Logical package grouping (e.g. "itil", "asset_management").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        package: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Genauigkeit.
        precision: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        /// Nachkommastellen.
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

    /// Atom mit beliebigem EDM-Typ, ohne weitere Einstellungen.
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
            unit_scale_field: None,
            code_list: None,
            link: false,
            multiline: false,
            side_effects: Vec::new(),
            ui: None,
        }
    }

    /// Zeichenkette mit maximaler Laenge.
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
            unit_scale_field: None,
            code_list: None,
            link: false,
            multiline: false,
            side_effects: Vec::new(),
            ui: None,
        }
    }

    /// Ganzzahl (`Edm.Int32`).
    pub fn int(name: &str, label: &str) -> Self {
        FieldSpec::atom(name, label, "Edm.Int32")
    }

    /// Wahrheitswert (`Edm.Boolean`).
    pub fn bool_field(name: &str, label: &str) -> Self {
        FieldSpec::atom(name, label, "Edm.Boolean")
    }

    /// Dezimalzahl mit Genauigkeit und Nachkommastellen.
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
            unit_scale_field: None,
            code_list: None,
            link: false,
            multiline: false,
            side_effects: Vec::new(),
            ui: None,
        }
    }

    /// Messgroesse, deren Einheit in `unit_field` steht.
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

    /// Markiert das Feld als berechnet (nicht schreibbar).
    pub fn computed(mut self) -> Self {
        if let FieldSpec::Atom { computed, .. } = &mut self {
            *computed = true;
        }
        self
    }

    /// Markiert das Feld als unveraenderlich nach dem Anlegen.
    pub fn immutable(mut self) -> Self {
        if let FieldSpec::Atom { immutable, .. } = &mut self {
            *immutable = true;
        }
        self
    }

    /// Ordnet das Feld einem Paket zu.
    pub fn pkg(mut self, p: &str) -> Self {
        match &mut self {
            FieldSpec::Atom { package, .. } => *package = Some(p.into()),
            FieldSpec::Measure { package, .. } => *package = Some(p.into()),
            _ => todo!(),
        }
        self
    }

    /// Verknuepft das Feld mit einer Festwertliste oder Wertehilfe.
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

    /// Returns the target entity of a lookup field, if applicable.
    ///
    /// # Returns
    ///
    /// * `Some(&str)` - The target entity name if this is a lookup field.
    /// * `None` - If this is not a lookup field.
    pub fn lookup_entity(&self) -> Option<&str> {
        match self {
            FieldSpec::Lookup { entity, .. } => Some(entity),
            _ => None,
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
    /// Draft-Unterstuetzung; bestehende Dictionaries bleiben draft-faehig.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub(crate) draft_enabled: bool,
    /// Entity name (e.g. "Orders"). Must match the entity name used in Relationships.
    pub(crate) name: String,

    /// Stammdaten-Startzeilen (Feldname -> Wert). Werden beim Start eingespielt, sofern
    /// die Entity noch keine Zeile mit gleichem Schluessel hat (siehe `seed_entities`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) seed: Vec<BTreeMap<String, serde_json::Value>>,

    /// Logical package grouping (e.g. "itil", "asset_management").
    pub(crate) package: String,

    /// Singular display name for the Object Page header (e.g. "Customer").
    /// Frei definierbar, unabhaengig vom Entitaetsnamen. Fehlt er, gilt der Name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) type_name: Option<String>,

    /// Plural display name (e.g. "Orders"). Default: set_name.
    pub(crate) type_name_plural: String,

    /// The entity's own fields (leaves). FK fields from Relationships are auto-generated
    /// and should NOT be declared here.
    pub(crate) fields: Vec<FieldSpec>,

    /// Benannte Listenansichten (Spaltenauswahl). Leer = Standard (nur `name`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) list_views: Vec<ListView>,

    /// Vorbelegte Filterwerte je Atom-Feld (`Common.FilterDefaultValue`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) filter_defaults: BTreeMap<String, FilterDefault>,

    /// Felder der Filterleiste in dieser Reihenfolge (`UI.SelectionFields`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) selection_fields: Vec<String>,

    /// Einstellungen je Object-Page-Facette (Tabellenabschnitt einer to-many Navigation).
    /// Fehlt ein Eintrag, gelten die Standardwerte.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) facets: Vec<FacetSpec>,

    /// Auditierung (Owner, CreatedBy/At, ChangedBy/At). Standard: `true`.
    /// `false` fuer Entitaeten wie Festwerte, die keinen Owner brauchen.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub(crate) audited: bool,

    /// Anzeige-Einstellungen der Entitaet (`ui`-Block in `ddic.json`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) ui: Option<EntityUi>,

    /// Einzelnes Feld als Primaerschluessel (Name eines Atoms). Fehlt es, gilt `ID` (Edm.Guid).
    /// Typ und Laenge kommen aus dem Atom. Nur ohne Drafts zulaessig, sonst wuerde
    /// `IsActiveEntity` Teil des Schluessels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) primary_key: Option<String>,
}

fn default_true() -> bool {
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
    pub(crate) facet: String,
    /// Listenansicht der Zielentitaet. Fehlt sie oder ist `default`, gilt das Standard-LineItem.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) view: Option<String>,
    /// Titel des Facetten-Abschnitts. Fehlt er, wird der Schluessel verwendet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    /// Felder des Abschnitts in Anzeigereihenfolge (Atome oder einwertige Navigationen).
    /// Bei `General` ohne Angabe: alle Felder. Bei `Default`: nicht genannte Felder.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) fields: Vec<String>,
}

/// Konstanter Standardwert fuer einen Filter. Zahlen und Datumswerte sind
/// bewusst nicht enthalten (eigener Literal-Typ noetig).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FilterDefault {
    /// Konstanter Text, z. B. `"Demo"`.
    Text(String),
    /// Konstanter Wahrheitswert.
    Flag(bool),
}

/// Eine benannte Listenansicht einer Entitaet.
///
/// `name = "default"` ist die Standardansicht (`UI.LineItem` ohne Qualifier).
/// Jede andere Ansicht erzeugt `UI.LineItem#<name>` und `UI.PresentationVariant#<name>`.
/// `role` ist vorbereitet fuer eine rollenbasierte Auswahl; heute ohne Wirkung.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListView {
    pub(crate) name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    /// Geordnete Spalten. Siehe `ListColumn`.
    pub(crate) columns: Vec<ListColumn>,
}

impl ListView {
    /// Name der Standardansicht.
    pub const DEFAULT: &'static str = "default";

    /// Ansicht aus reinen Feldnamen (ohne Breite und Prioritaet).
    pub fn new(name: impl Into<String>, columns: &[&str]) -> Self {
        Self {
            name: name.into(),
            role: None,
            columns: columns.iter().map(|c| ListColumn::new(*c)).collect(),
        }
    }

    /// Ansicht aus fertig konfigurierten Spalten.
    pub fn with_columns(name: impl Into<String>, columns: Vec<ListColumn>) -> Self {
        Self {
            name: name.into(),
            role: None,
            columns,
        }
    }

    /// `true`, wenn dies die Standardansicht (`default`) ist.
    pub fn is_default(&self) -> bool {
        self.name == Self::DEFAULT
    }
}

/// Wichtigkeit einer Listenspalte. Wird als `UI.Importance` an das Feld geschrieben;
/// Fiori Elements blendet Spalten mit niedriger Prioritaet bei schmalen Bildschirmen zuerst aus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    /// Hohe Wichtigkeit: bleibt bei schmalen Bildschirmen sichtbar.
    High,
    /// Mittlere Wichtigkeit.
    Medium,
    /// Niedrige Wichtigkeit: wird bei schmalen Bildschirmen zuerst ausgeblendet.
    Low,
}

impl Priority {
    /// Name des Enum-Members in `UI.ImportanceType`.
    pub fn importance_member(self) -> &'static str {
        match self {
            Priority::High => "UI.ImportanceType/High",
            Priority::Medium => "UI.ImportanceType/Medium",
            Priority::Low => "UI.ImportanceType/Low",
        }
    }
}

/// Eine Spalte einer Listenansicht.
///
/// * `field`: Atom (`file_size`) oder einwertige Navigation (`agent`, Anzeige `agent/name`).
/// * `width`: CSS-Breite wie `120px`, `15rem`, `20%` oder `auto`. Ohne Angabe: Standardbreite.
/// * `priority`: `high`, `medium` oder `low`. Ohne Angabe: keine Annotation (Standard).
///
/// In `ddic.json` akzeptiert die Struktur auch die Kurzform eines blossen Feldnamens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "ListColumnInput")]
pub struct ListColumn {
    pub(crate) field: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) width: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) priority: Option<Priority>,
}

impl ListColumn {
    /// Spalte fuer ein Feld ohne Breite und Prioritaet.
    pub fn new(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            width: None,
            priority: None,
        }
    }

    /// Setzt die CSS-Breite der Spalte (Builder).
    pub fn with_width(mut self, width: impl Into<String>) -> Self {
        self.width = Some(width.into());
        self
    }

    /// Setzt die Prioritaet der Spalte (Builder).
    pub fn with_priority(mut self, priority: Priority) -> Self {
        self.priority = Some(priority);
        self
    }
}

/// Eingabeform einer Listenspalte: Feldname oder Objekt.
#[derive(Deserialize)]
#[serde(untagged)]
enum ListColumnInput {
    Field(String),
    Detailed {
        field: String,
        #[serde(default)]
        width: Option<String>,
        #[serde(default)]
        priority: Option<Priority>,
    },
}

impl From<ListColumnInput> for ListColumn {
    fn from(input: ListColumnInput) -> Self {
        match input {
            ListColumnInput::Field(field) => ListColumn::new(field),
            ListColumnInput::Detailed {
                field,
                width,
                priority,
            } => ListColumn {
                field,
                width,
                priority,
            },
        }
    }
}

/// An explicit facet section for the Object Page.
#[derive(Debug, Clone)]
pub struct FacetSectionSpec {
    pub(crate) label: String,
    pub(crate) id: String,
    pub(crate) field_group_qualifier: String,
    pub(crate) field_group_label: String,
}

/// An explicit table facet (child collection table on the Object Page).
#[derive(Debug, Clone)]
pub struct TableFacetSpec {
    pub(crate) label: String,
    pub(crate) id: String,
    pub(crate) navigation_property: String,
}

impl ValueListFilter {
    /// Feld der Wertehilfe, das lokal (in der Entitaet) gefiltert wird.
    pub fn local_property(&self) -> &str {
        &self.local_property
    }

    /// Feld der Zielentitaet der Wertehilfe.
    pub fn target_property(&self) -> &str {
        &self.target_property
    }
}

impl ConstantFilter {
    /// Feld der Zielentitaet, auf das ein konstanter Wert gesetzt wird.
    pub fn target_property(&self) -> &str {
        &self.target_property
    }

    /// Konstanter Wert des Filters.
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl FieldUi {
    /// Anzeigeformat des Feldes, falls gesetzt.
    pub fn format(&self) -> Option<DisplayFormat> {
        self.format
    }
}

impl EntityUi {
    /// Felder, die in der Oberflaeche ausgeblendet werden.
    pub fn hidden_fields(&self) -> &[String] {
        &self.hidden_fields
    }

    /// Abschnitte der Object Page.
    pub fn sections(&self) -> &[UiSection] {
        &self.sections
    }

    /// Facetten, die ausgeblendet werden.
    pub fn hide_facets(&self) -> &[String] {
        &self.hide_facets
    }

    /// Zusaetzliche Spalten der Liste.
    pub fn list_columns(&self) -> &[UiListColumn] {
        &self.list_columns
    }

    /// Zusaetzliche Listen-Templates.
    pub fn list_templates(&self) -> &[UiListTemplate] {
        &self.list_templates
    }
}

impl UiSection {
    /// Schluessel des Abschnitts.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Titel des Abschnitts.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Fragment-Name (ohne Namensraum).
    pub fn fragment(&self) -> &str {
        &self.fragment
    }

    /// Anker des Abschnitts, falls gesetzt.
    pub fn anchor(&self) -> Option<&str> {
        self.anchor.as_deref()
    }

    /// Platzierung des Abschnitts, falls gesetzt.
    pub fn placement(&self) -> Option<&str> {
        self.placement.as_deref()
    }
}

impl UiListColumn {
    /// Schluessel der Spalte.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Ueberschrift der Spalte.
    pub fn header(&self) -> &str {
        &self.header
    }

    /// Fragment-Name der Spalte (ohne Namensraum).
    pub fn fragment(&self) -> &str {
        &self.fragment
    }

    /// Anker der Spalte.
    pub fn anchor(&self) -> &str {
        &self.anchor
    }

    /// Platzierung der Spalte, falls gesetzt.
    pub fn placement(&self) -> Option<&str> {
        self.placement.as_deref()
    }

    /// CSS-Breite der Spalte, falls gesetzt.
    pub fn width(&self) -> Option<&str> {
        self.width.as_deref()
    }
}

impl UiListTemplate {
    /// Schluessel des Listen-Templates.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Anzeige-Bezeichnung des Templates.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Fragment-Name des Templates (ohne Namensraum).
    pub fn fragment(&self) -> &str {
        &self.fragment
    }
}

impl FacetSpec {
    /// Neue Facetten-Einstellung fuer `facet` (Identifikator, siehe `facet()`).
    pub fn new(
        facet: impl Into<String>,
        view: Option<String>,
        title: Option<String>,
        fields: Vec<String>,
    ) -> Self {
        Self {
            facet: facet.into(),
            view,
            title,
            fields,
        }
    }

    /// Identifikator der Facette (Navigation, `General`, `Default` oder frei gewaehlt).
    pub fn facet(&self) -> &str {
        &self.facet
    }

    /// Setzt die Listenansicht der Zielentitaet (`None` = Standard).
    pub fn set_view(&mut self, view: Option<String>) {
        self.view = view;
    }

    /// Listenansicht der Zielentitaet, falls gesetzt.
    pub fn view(&self) -> Option<&str> {
        self.view.as_deref()
    }

    /// Titel des Abschnitts, falls gesetzt.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Felder des Abschnitts in Anzeigereihenfolge.
    pub fn fields(&self) -> &[String] {
        &self.fields
    }
}

impl FacetSectionSpec {
    /// Bezeichnung des Abschnitts.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Technische ID des Abschnitts.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Qualifier der Feldgruppe.
    pub fn field_group_qualifier(&self) -> &str {
        &self.field_group_qualifier
    }

    /// Bezeichnung der Feldgruppe.
    pub fn field_group_label(&self) -> &str {
        &self.field_group_label
    }
}

impl TableFacetSpec {
    /// Bezeichnung der Tabelle.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Technische ID der Tabelle.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Navigationsproperty, deren Werte die Tabelle zeigt.
    pub fn navigation_property(&self) -> &str {
        &self.navigation_property
    }
}

impl ListView {
    /// Name der Listenansicht.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Rolle der Ansicht, falls gesetzt (heute ohne Wirkung).
    pub fn role(&self) -> Option<&str> {
        self.role.as_deref()
    }

    /// Geordnete Spalten der Ansicht.
    pub fn columns(&self) -> &[ListColumn] {
        &self.columns
    }
}

impl ListColumn {
    /// Feld oder Navigationspfad der Spalte.
    pub fn field(&self) -> &str {
        &self.field
    }

    /// CSS-Breite der Spalte, falls gesetzt.
    pub fn width(&self) -> Option<&str> {
        self.width.as_deref()
    }

    /// Prioritaet der Spalte, falls gesetzt.
    pub fn priority(&self) -> Option<Priority> {
        self.priority
    }
}

impl EntitySpec {
    /// `true`, wenn die Entitaet Drafts unterstuetzt.
    pub fn draft_enabled(&self) -> bool {
        self.draft_enabled
    }

    /// Stammdaten-Startzeilen.
    pub fn seed(&self) -> &[BTreeMap<String, serde_json::Value>] {
        &self.seed
    }

    /// Plural-Anzeigename.
    pub fn type_name_plural(&self) -> &str {
        &self.type_name_plural
    }

    /// `true`, wenn die Entitaet auditiert wird.
    pub fn audited(&self) -> bool {
        self.audited
    }

    /// Anzeige-Einstellungen der Entitaet, falls gesetzt.
    pub fn ui(&self) -> Option<&EntityUi> {
        self.ui.as_ref()
    }

    /// Name des Primaerschluessel-Feldes, falls gesetzt.
    pub fn primary_key(&self) -> Option<&str> {
        self.primary_key.as_deref()
    }

    /// Schaltet Drafts ein oder aus.
    pub fn set_draft_enabled(&mut self, enabled: bool) {
        self.draft_enabled = enabled;
    }

    /// Schaltet die Auditierung ein oder aus.
    pub fn set_audited(&mut self, audited: bool) {
        self.audited = audited;
    }

    /// Setzt den Singular-Anzeigenamen (`None` = Entitaetsname).
    pub fn set_type_name(&mut self, type_name: Option<String>) {
        self.type_name = type_name;
    }

    /// Ersetzt die Felder der Filterleiste.
    pub fn set_selection_fields(&mut self, fields: Vec<String>) {
        self.selection_fields = fields;
    }

    /// Haengt ein Feld an die Feldliste an.
    pub fn add_field(&mut self, field: FieldSpec) {
        self.fields.push(field);
    }

    /// Veraendert die Felder in place (z. B. um UI-Einstellungen zu setzen).
    pub fn update_fields(&mut self, mut f: impl FnMut(&mut FieldSpec)) {
        self.fields.iter_mut().for_each(|field| f(field));
    }

    /// Ersetzt alle Object-Page-Facetten.
    pub fn set_facets(&mut self, facets: Vec<FacetSpec>) {
        self.facets = facets;
    }

    /// Setzt den Filterstandard eines Atom-Feldes.
    pub fn set_filter_default(&mut self, field: impl Into<String>, value: FilterDefault) {
        self.filter_defaults.insert(field.into(), value);
    }

    /// Haengt eine Facette an die Object-Page-Einstellungen an.
    pub fn add_facet(&mut self, facet: FacetSpec) {
        self.facets.push(facet);
    }

    /// Sucht das Feld der Entität heraus und gibt die Spec dazu zurück, wenn gefunden.
    ///
    /// # Arguments
    ///
    /// * `column_name` - Der Name des Feldes, das gesucht werden soll.
    ///
    /// # Returns
    ///
    /// * `Some(&FieldSpec)` - Die Spezifikation des Feldes, falls gefunden.
    /// * `None` - Wenn kein Feld mit dem angegebenen Namen existiert.
    pub fn field_by_name(&self, column_name: &str) -> Option<&FieldSpec> {
        self.fields().find(|f| f.name() == column_name)
    }
}

impl EntitySpec {
    /// Erzeugt eine Entitaet mit Drafts und Audit (beides aktiv) ohne Felder.
    pub fn new(
        name: impl Into<String>,
        package: impl Into<String>,
        type_name_plural: impl Into<String>,
    ) -> Self {
        Self {
            draft_enabled: true,
            name: name.into(),
            seed: vec![],
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
            primary_key: None,
        }
    }

    /// Macht ein Atom-Feld zum Primaerschluessel (siehe `EntitySpec::primary_key`).
    pub fn with_primary_key(mut self, field: impl Into<String>) -> Self {
        self.primary_key = Some(field.into());
        self
    }

    /// Name des Primaerschluessel-Feldes; ohne Angabe `ID`.
    pub fn primary_key_name(&self) -> &str {
        self.primary_key.as_deref().unwrap_or("ID")
    }

    /// EDM-Typ des Primaerschluessels. Ohne eigenes Feld `Edm.Guid`.
    pub fn primary_key_type(&self) -> &str {
        match &self.primary_key {
            None => "Edm.Guid",
            Some(name) => self
                .fields()
                .find(|f| f.name() == name)
                .map(|f| f.edm_type())
                .unwrap_or("Edm.Guid"),
        }
    }

    /// Schaltet die Auditierung dieser Entitaet ab (kein Owner, keine Audit-Felder).
    pub fn without_audit(mut self) -> Self {
        self.audited = false;
        self
    }

    /// Schaltet Drafts fuer diese Entitaet ab (Builder).
    pub fn without_drafts(mut self) -> Self {
        self.draft_enabled = false;
        self
    }

    /// Schluesselfelder fuer OData und Pfadsegmente: eigener Primaerschluessel oder
    /// `ID` (+ `IsActiveEntity` bei Drafts).
    pub fn key_fields(&self) -> Vec<&str> {
        match &self.primary_key {
            Some(pk) => vec![pk.as_str()],
            None => self.key_names().to_vec(),
        }
    }

    /// Namen der Schluesselfelder.
    pub fn key_names(&self) -> &[&str] {
        if self.draft_enabled {
            &["ID", "IsActiveEntity"]
        } else {
            &["ID"]
        }
    }

    /// `true`, wenn die Entitaet auditiert wird.
    pub fn is_audited(&self) -> bool {
        self.audited
    }

    /// Facetten-Einstellungen der Object Page.
    pub fn facets(&self) -> &[FacetSpec] {
        &self.facets
    }

    /// Facette mit dem gegebenen Schluessel, falls konfiguriert.
    pub fn facet(&self, key: &str) -> Option<&FacetSpec> {
        self.facets.iter().find(|f| f.facet == key)
    }

    /// Felder der Filterleiste in Reihenfolge.
    pub fn selection_fields(&self) -> &[String] {
        &self.selection_fields
    }

    /// Vorbelegte Filterwerte je Feld.
    pub fn filter_defaults(&self) -> &BTreeMap<String, FilterDefault> {
        &self.filter_defaults
    }

    /// Fuegt eine Listenansicht hinzu (Builder).
    pub fn list_view(mut self, view: ListView) -> Self {
        self.list_views.push(view);
        self
    }

    /// Alle benannten Listenansichten.
    pub fn list_views(&self) -> &[ListView] {
        &self.list_views
    }

    /// Fuegt ein Feld hinzu (Builder).
    pub fn field(mut self, fld: FieldSpec) -> Self {
        debug!("add field {}", fld.name());
        self.fields.push(fld);
        self
    }

    /// Name der Entitaet.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Alle eigenen Felder der Entitaet.
    pub fn fields(&self) -> impl Iterator<Item = &FieldSpec> {
        self.fields.iter()
    }

    /// Singularer Anzeigename: `type_name`, sonst der Entitaetsname.
    pub fn type_name(&self) -> &str {
        self.type_name.as_deref().unwrap_or(&self.name)
    }

    /// Name des EntitySets fuer OData (`{name}Set`).
    pub fn setname(&self) -> String {
        format!("{}Set", self.name)
    }
}

#[cfg(test)]
mod ui_block_tests;
