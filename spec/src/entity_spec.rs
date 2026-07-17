
/// How a measure field relates to its unit/currency companion.
#[derive(Debug, Clone)]
pub enum MeasureKind {
    /// `Org.OData.Measures.V1.ISOCurrency Path="{unit_field}"`
    Currency,
    /// `Org.OData.Measures.V1.Unit Path="{unit_field}"`
    Unit,
}

/// A context filter for value lists: "my local field X must match target field Y".
/// Generates a `Common.ValueListParameterIn` in the OData annotation.
#[derive(Debug, Clone)]
pub struct ValueListFilter {
    /// Property on the local entity (e.g. "ConfigID", "ID")
    pub local_property: String,
    /// Property on the value list target entity (e.g. "ConfigID")
    pub target_property: String,
}

/// Value help configuration for an Atom field.
#[derive(Debug, Clone)]
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

/// Presentation overrides for a field. All fields are optional — `None` means
/// "use smart defaults" (non-computed non-Guid fields visible in list & form,
/// form_group defaults to "General", list order follows definition order).
#[derive(Debug, Clone, Default)]
pub struct PresentationOverrides {
    /// Field appears as filter control in the list header bar.
    pub searchable: Option<bool>,
    /// Field appears as a column in the list/table view.
    pub show_in_list: Option<bool>,
    /// Column order in the list (ascending).
    pub list_sort_order: Option<u32>,
    /// Responsive table priority ("High", "Medium", "Low").
    pub list_importance: Option<String>,
    /// Path to a criticality indicator field (e.g. "StatusCriticality").
    pub criticality_path: Option<String>,
    /// FieldGroup qualifier — groups this field into a form section.
    pub form_group: Option<String>,
}

/// A field specification — describes the entity's own data (leaves on the relationship tree).
///
/// Only two variants: Atom (simple values) and Measure (value + unit/currency dependency).
/// References to other entities are expressed as `Relationship`, not as fields.
#[derive(Debug, Clone)]
pub enum FieldSpec {
    /// Simple value: text, number, date, boolean, coded value with optional dropdown.
    Atom {
        name: String,
        label: String,
        edm_type: String,
        /// Logical package grouping (e.g. "itil", "asset_management").
        package: Option<String>,
        max_length: Option<u32>,
        precision: Option<u32>,
        scale: Option<u32>,
        /// Server-generated field — never editable (Core.Computed + NonInsertable).
        computed: bool,
        /// Set at creation, read-only afterward (Core.Immutable).
        immutable: bool,
        /// Optional value help (dropdown/dialog).
        value_list: Option<AtomValueList>,
        /// UI presentation overrides.
        presentation: PresentationOverrides,
    },
    /// Amount/quantity with a unit/currency companion field.
    /// Generates `Org.OData.Measures.V1.ISOCurrency` or `.Unit` annotation.
    Measure {
        name: String,
        label: String,
        /// Logical package grouping (e.g. "itil", "asset_management").
        package: Option<String>,
        precision: Option<u32>,
        scale: Option<u32>,
        /// Name of the companion Atom field holding the unit/currency code (e.g. "Currency").
        unit_field: String,
        /// Whether this is a currency amount or a physical quantity.
        kind: MeasureKind,
        /// UI presentation overrides.
        presentation: PresentationOverrides,
    },
}


impl FieldSpec {
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
            presentation: PresentationOverrides::default(),
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
            presentation: PresentationOverrides::default(),
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
            presentation: PresentationOverrides::default(),
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
            presentation: PresentationOverrides::default(),
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

    pub fn form_group(mut self, group: &str) -> Self {
        match &mut self {
            FieldSpec::Atom { presentation, .. } => presentation.form_group = Some(group.into()),
            FieldSpec::Measure { presentation, .. } => presentation.form_group = Some(group.into()),
        }
        self
    }

    pub fn searchable(mut self) -> Self {
        match &mut self {
            FieldSpec::Atom { presentation, .. } => presentation.searchable = Some(true),
            FieldSpec::Measure { presentation, .. } => presentation.searchable = Some(true),
        }
        self
    }

    pub fn show_in_list(mut self) -> Self {
        match &mut self {
            FieldSpec::Atom { presentation, .. } => presentation.show_in_list = Some(true),
            FieldSpec::Measure { presentation, .. } => presentation.show_in_list = Some(true),
        }
        self
    }

    pub fn pkg(mut self, p: &str) -> Self {
        match &mut self {
            FieldSpec::Atom { package, .. } => *package = Some(p.into()),
            FieldSpec::Measure { package, .. } => *package = Some(p.into()),
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
        }
    }

    /// Returns the field label regardless of variant.
    pub fn label(&self) -> &str {
        match self {
            FieldSpec::Atom { label, .. } => label,
            FieldSpec::Measure { label, .. } => label,
        }
    }

    /// Returns the package assignment regardless of variant.
    pub fn package(&self) -> Option<&str> {
        match self {
            FieldSpec::Atom { package, .. } => package.as_deref(),
            FieldSpec::Measure { package, .. } => package.as_deref(),
        }
    }

    /// Returns the EDM type regardless of variant.
    pub fn edm_type(&self) -> &str {
        match self {
            FieldSpec::Atom { edm_type, .. } => edm_type,
            FieldSpec::Measure { .. } => "Edm.Decimal",
        }
    }

    /// Returns the presentation overrides regardless of variant.
    pub fn presentation(&self) -> &PresentationOverrides {
        match self {
            FieldSpec::Atom { presentation, .. } => presentation,
            FieldSpec::Measure { presentation, .. } => presentation,
        }
    }
}

/// Application-level entity specification — describes an entity's own fields and header layout.
///
/// Entities may be introduced by `Relationship` declarations without an explicit `EntitySpec`.
/// In that case they get default fields (ID + Name). An `EntitySpec` refines the entity
/// with additional fields, header info, and data points.
#[derive(Debug, Clone)]
pub struct EntitySpec {
    /// Entity name (e.g. "Orders"). Must match the entity name used in Relationships.
    pub name: String,

    /// Logical package grouping (e.g. "itil", "asset_management").
    pub package: String,

    /// Plural display name (e.g. "Orders"). Default: set_name.
    pub type_name_plural: String,

    /// The entity's own fields (leaves). FK fields from Relationships are auto-generated
    /// and should NOT be declared here.
    pub fields: Vec<FieldSpec>,
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
            type_name_plural: type_name_plural.into(),
            fields: vec![],
        }
    }

    pub fn field(mut self, fld: FieldSpec) -> Self {
        self.fields.push(fld);
        self
    }
    
    pub fn name(&self) -> &str {
        &self.name
    }
    
    pub fn fields(&self) -> impl Iterator<Item = &FieldSpec> {
        self.fields.iter()
    }
    
    pub fn setname(&self) -> String {
        format!("{}Set", self.name)
    }   
}
