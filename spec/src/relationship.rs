use serde::{Deserialize, Serialize};

/// Eine Seite einer Beziehung: identifiziert die Entitaet und den Namen der Navigationsproperty.
#[derive(Debug, Clone)]
pub struct Side {
    /// Name der EntitySet (z. B. "Customers", "Orders")
    pub(crate) entity: String,
    /// Name der Navigationsproperty auf dieser Entitaet (z. B. "Orders" bei Customers, "Customer" bei Orders).
    /// Mit `_` prefixen, um die TableFacet-Generierung zu unterdruecken (Navigation gibt es im EDMX fuer $expand, ist im UI aber versteckt).
    pub(crate) nav_name: String,
}

impl Side {
    /// Erzeugt eine Seite mit Entitaet und Navigationsnamen.
    pub fn new(entity: &str, nav_name: &str) -> Self {
        Side {
            entity: entity.into(),
            nav_name: nav_name.into(),
        }
    }

    /// Name der Entitaet (EntitySet).
    pub fn entity(&self) -> &str {
        &self.entity
    }

    /// Name der Navigationsproperty.
    pub fn nav_name(&self) -> &str {
        &self.nav_name
    }
}

impl ConditionalRef {
    /// Bedingungstyp.
    pub fn condition(&self) -> &Condition {
        &self.condition
    }

    /// Name der referenzierten Beziehung.
    pub fn reference(&self) -> &str {
        &self.reference
    }
}

/// Ein Bedingungstyp, der eine Beziehung auf eine Teilmenge einer anderen beengt.
#[derive(Debug, Clone)]
pub enum Condition {
    // Waehle eine Entitaet aus der Sammlung der referenzierten Beziehung.
    // Der Wertebereich wird automatisch gefiltert: `(parent ID) = (child FK)`.
    /// Die Teilmenge ist die Sammlung der referenzierten Beziehung (Fremdschluessel muss dort liegen).
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
    pub(crate) condition: Condition,
    /// Name der Beziehung aus deren Sammlung diese waehlt.
    pub(crate) reference: String,
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
    pub(crate) name: String,
    /// The "1" side — gets a Collection NavigationProperty + TableFacet
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) one: Option<String>,
    /// FK field name on the "many" entity. Default: `"{many.nav_name}ID"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) one_field: Option<String>,
    /// The "N" side — gets a FK property + 1:1 NavigationProperty + ValueList + SemanticObject
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) many: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) many_property: Option<String>,
    /// `true` = composition (parent controls child lifecycle, child gets DraftNode).
    /// `false` = association (independent entities, just linked).
    #[serde(default, skip_serializing_if = "crate::entity_spec::is_false")]
    pub(crate) owned: bool,
    /// Logical package grouping (e.g. "itil", "asset_management").
    pub(crate) package: String,
    /// Lesbarer Titel der Sammelnavigation auf der "one"-Seite (z. B. "Video tags").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) one_label: Option<String>,
    /// Lesbarer Titel der Einzelnavigation auf der "many"-Seite (z. B. "Video").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) many_label: Option<String>,
}

impl Relationship {
    /// Erzeugt eine Beziehung ohne Seiten. Die Seiten werden ueber `with_one` und `with_many` gesetzt.
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

    /// Setzt die "one"-Seite (Entitaet und FK-Feld) als Builder.
    pub fn with_one(mut self, entity: impl Into<String>, property: impl Into<String>) -> Self {
        self.one = Some(entity.into());
        self.one_field = Some(property.into());
        self
    }

    /// Setzt die "many"-Seite (Entitaet und Navigationsproperty) als Builder.
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

    /// `true`, wenn die Beziehung eine Komposition ist.
    pub fn is_owned(&self) -> bool {
        self.owned
    }

    /// Eindeutiger Name der Beziehung.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Paket der Beziehung.
    pub fn package(&self) -> &str {
        &self.package
    }

    /// Entitaet der "one"-Seite, falls gesetzt.
    pub fn one_opt(&self) -> Option<&str> {
        self.one.as_deref()
    }

    /// FK-Feldname auf der "many"-Seite, falls gesetzt.
    pub fn one_field_opt(&self) -> Option<&str> {
        self.one_field.as_deref()
    }

    /// Entitaet der "many"-Seite, falls gesetzt.
    pub fn many_opt(&self) -> Option<&str> {
        self.many.as_deref()
    }

    /// Navigationsproperty der "many"-Seite, falls gesetzt.
    pub fn many_property_opt(&self) -> Option<&str> {
        self.many_property.as_deref()
    }

    /// Titel der Sammelnavigation auf der "one"-Seite, falls gesetzt.
    pub fn one_label(&self) -> Option<&str> {
        self.one_label.as_deref()
    }

    /// Titel der Einzelnavigation auf der "many"-Seite, falls gesetzt.
    pub fn many_label(&self) -> Option<&str> {
        self.many_label.as_deref()
    }

    /// Setzt, ob die Beziehung eine Komposition ist.
    pub fn set_owned(&mut self, owned: bool) {
        self.owned = owned;
    }

    /// Setzt den Titel der Sammelnavigation.
    pub fn set_one_label(&mut self, label: Option<String>) {
        self.one_label = label;
    }

    /// Setzt den Titel der Einzelnavigation.
    pub fn set_many_label(&mut self, label: Option<String>) {
        self.many_label = label;
    }

    /// Entitaet der "one"-Seite. Panics, wenn nicht gesetzt (siehe `one_opt`).
    pub fn one(&self) -> &str {
        self.one.as_ref().unwrap()
    }

    /// Entitaet der "many"-Seite. Panics, wenn nicht gesetzt (siehe `many_opt`).
    pub fn many(&self) -> &str {
        &self.many.as_ref().unwrap()
    }

    /// FK-Feld auf der "many"-Seite. Panics, wenn nicht gesetzt (siehe `one_field_opt`).
    pub fn one_property(&self) -> &str {
        &self.one_field.as_ref().unwrap()
    }

    /// Navigationsproperty der "many"-Seite. Panics, wenn nicht gesetzt (siehe `many_property_opt`).
    pub fn many_property(&self) -> &str {
        &self.many_property.as_ref().unwrap()
    }
}
