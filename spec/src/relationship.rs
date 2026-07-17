/// A side of a relationship — identifies the entity and its navigation property name.
#[derive(Debug, Clone)]
pub struct Side {
    /// EntitySet name (e.g. "Customers", "Orders")
    pub entity: String,
    /// Navigation property name on this entity (e.g. "Orders" on Customers, "Customer" on Orders).
    /// Prefix with `_` to suppress TableFacet generation (nav exists in EDMX for $expand but is hidden in UI).
    pub nav_name: String,
}

impl Side {
    pub fn new(entity: &str, nav_name: &str) -> Self {
        Side {
            entity: entity.into(),
            nav_name: nav_name.into(),
        }
    }
}

/// A condition type that narrows a relationship to a subset of another.
#[derive(Debug, Clone)]
pub enum Condition {
    /// Pick one entity from the collection of the referenced relationship.
    /// The value list is automatically filtered: `(parent ID) = (child FK)`.
    SubsetOf,
    // Future: Where { field: String, value: String },
}

/// A conditional reference — "this relationship picks from another relationship's collection."
///
/// When present on a `Relationship`, it means the FK doesn't point to an independent
/// entity but to a member of another relationship's collection. The value list is
/// automatically filtered by the referenced relationship's FK.
#[derive(Debug, Clone)]
pub struct ConditionalRef {
    /// The condition type.
    pub condition: Condition,
    /// Name of the relationship whose collection this picks from.
    pub reference: String,
}

/// A first-class relationship between two entities.
///
/// Each relationship has a unique `name` for referencing (e.g. from `ConditionalRef`).
/// One declaration creates BOTH directions:
/// - On the "many" side: FK property, 1:1 NavigationProperty, value list
/// - On the "one" side: 1:N NavigationProperty, TableFacet on Object Page
///
/// When `condition` is set, this relationship is a subset-pick from another
/// relationship's collection. The FK value list is automatically filtered.
///
/// # Examples
///
/// ```ignore
/// // Composition
/// Relationship {
///     name: "EntityConfig_EntityFields".into(),
///     one:  Side::new("EntityConfigs", "Fields"),
///     many: Side::new("EntityFields", "_Config"),
///     owned: true,
///     ..
/// }
///
/// // Conditional: pick one from the Fields composition
/// Relationship {
///     name: "HeaderTitle".into(),
///     one:  Side::new("EntityFields", "_HeaderTitleField"),
///     many: Side::new("EntityConfigs", "_HeaderTitlePath"),
///     owned: false,
///     fk_field: Some("HeaderTitlePath".into()),
///     fk_label: Some("Header Title Field".into()),
///     condition: Some(ConditionalRef {
///         condition: Condition::SubsetOf,
///         reference: "EntityConfig_EntityFields".into(),
///     }),
///     ..
/// }
/// ```
#[derive(Debug, Clone)]
pub struct Relationship {
    /// Unique name for this relationship (used as reference target by ConditionalRef).
    pub name: String,
    /// The "1" side — gets a Collection NavigationProperty + TableFacet
    pub one: String,
    /// The "N" side — gets a FK property + 1:1 NavigationProperty + ValueList + SemanticObject
    pub many: String,
    /// `true` = composition (parent controls child lifecycle, child gets DraftNode).
    /// `false` = association (independent entities, just linked).
    pub owned: bool,
    /// FK field name on the "many" entity. Default: `"{many.nav_name}ID"`.
    pub one_field: String,
    /// Logical package grouping (e.g. "itil", "asset_management").
    pub package: String,
}

impl Relationship {
    pub fn new(
        name: impl Into<String>,
        one: impl Into<String>,
        many: impl Into<String>,
        one_field: impl Into<String>,
        package: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            one: one.into(),
            many: many.into(),
            owned: false,
            one_field: one_field.into(),
            package: package.into(),
        }
    }
    
    pub fn name(&self) -> &str {
        &self.name
    }
    
    pub fn one(&self) -> &str {
        &self.one
    }
    
    pub fn many(&self) -> &str {
        &self.many
    }
}
