use serde::{Deserialize, Serialize};

/// Eine Seite einer Beziehung: identifiziert die Entitaet und den Namen der Navigationsproperty.
#[derive(Debug, Clone)]
pub struct Side {
    /// Name der EntitySet (z. B. "Customers", "Orders")
    pub entity: String,
    /// Name der Navigationsproperty auf dieser Entitaet (z. B. "Orders" bei Customers, "Customer" bei Orders).
    /// Mit `_` prefixen, um die TableFacet-Generierung zu unterdruecken (Navigation gibt es im EDMX fuer $expand, ist im UI aber versteckt).
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

/// Ein Bedingungstyp, der eine Beziehung auf eine Teilmenge einer anderen beengt.
#[derive(Debug, Clone)]
pub enum Condition {
    // Waehle eine Entitaet aus der Sammlung der referenzierten Beziehung.
    // Der Wertebereich wird automatisch gefiltert: `(parent ID) = (child FK)`.
    SubsetOf,
    // Future: Where { field: String, value: String },
}

/// EinBedungsverweis: „dieseWaehlt aus der einer anderen Beziehung.Sammlung.
///
/// Wenn auf einer `Relationship` vorhanden, bedeutet das, dass der Fremdschluessel
/// nicht auf eine unabhaengige Entitaet, sondern auf ein Mitglied der Sammlung einer
/// anderen Beziehung zeigt. Der Wertebereich wird automatisch nach dem
/// Fremdschluessel der referenzierten Beziehung gefiltert.
#[derive(Debug, Clone)]
pub struct ConditionalRef {
    /// Der Bedingungstyp.
    pub condition: Condition,
    /// Name der Beziehung aus deren Sammlung diese waehlt.
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Relationship {
    /// Unique name for this relationship (used as reference target by ConditionalRef).
    pub name: String,
    /// The "1" side — gets a Collection NavigationProperty + TableFacet
    pub one: Option<String>,
    /// FK field name on the "many" entity. Default: `"{many.nav_name}ID"`.
    pub one_field: Option<String>,
    /// The "N" side — gets a FK property + 1:1 NavigationProperty + ValueList + SemanticObject
    pub many: Option<String>,
    pub many_property: Option<String>,
    /// `true` = composition (parent controls child lifecycle, child gets DraftNode).
    /// `false` = association (independent entities, just linked).
    #[serde(default, skip_serializing_if = "crate::entity_spec::is_false")]
    pub owned: bool,
    /// Logical package grouping (e.g. "itil", "asset_management").
    pub package: String,
    /// Lesbarer Titel der Sammelnavigation auf der "one"-Seite (z. B. "Video tags").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_label: Option<String>,
    /// Lesbarer Titel der Einzelnavigation auf der "many"-Seite (z. B. "Video").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub many_label: Option<String>,
}

impl Relationship {
    pub fn new(name: impl Into<String>, package: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            package: package.into(),
            one: None,
            one_field: None,
            many: None,
            many_property: None,
            owned: false,
            one_label: None,
            many_label: None,
        }
    }

    pub fn with_one(mut self, entity: impl Into<String>, property: impl Into<String>) -> Self {
        self.one = Some(entity.into());
        self.one_field = Some(property.into());
        self
    }

    pub fn with_many(mut self, entity: impl Into<String>, property: impl Into<String>) -> Self {
        self.many = Some(entity.into());
        self.many_property = Some(property.into());
        self
    }

    /// Mark this relationship as a composition: the many side lives inside the one side's draft.
    pub fn owned(mut self) -> Self {
        self.owned = true;
        self
    }

    pub fn is_owned(&self) -> bool {
        self.owned
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn one(&self) -> &str {
        self.one.as_ref().unwrap()
    }

    pub fn many(&self) -> &str {
        &self.many.as_ref().unwrap()
    }

    pub fn one_property(&self) -> &str {
        &self.one_field.as_ref().unwrap()
    }

    pub fn many_property(&self) -> &str {
        &self.many_property.as_ref().unwrap()
    }
}
