//! Explizite Metadaten-Schnittstelle der Stores.
//!
//! Bewusst **ausserhalb** von [`crate::DataStore`]: der Store-Trait beschreibt
//! die Datenebene, dieses Modul die Struktur. Dokumenten-Backends koennen die
//! Metadaten ignorieren, ein relationales Backend braucht sie vollstaendig -
//! deshalb ist das Modell total (keine `Option`-Wueste) und wird einmalig
//! validiert, statt bei jeder Query neu interpretiert zu werden.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::sync::Arc;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tracing::debug;

/// Schluesselspalte jedes Entity-Sets.
pub const ID_COLUMN: &str = "ID";
/// Diskriminator zwischen aktivem Satz und Draft.
pub const IS_ACTIVE_COLUMN: &str = "IsActiveEntity";
/// Suffix der Fremdschluesselspalten (`{navigation}_ID`).
pub const ID_SUFFIX: &str = "_ID";

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("Entity-Set '{0}' ist nicht im Schema definiert")]
    UnknownSet(String),

    #[error("Entity-Set '{set}' hat keine Spalte '{column}'")]
    UnknownColumn { set: String, column: String },

    #[error("Entity-Set '{set}' hat keine Navigation '{association}'")]
    UnknownAssociation { set: String, association: String },

    #[error("'{set}.{association}' verweist auf unbekanntes Set '{target}'")]
    DanglingTarget {
        set: String,
        association: String,
        target: String,
    },

    #[error("'{set}.{association}' braucht Fremdschluesselspalte '{column}' in '{owner}'")]
    MissingForeignKey {
        set: String,
        association: String,
        column: String,
        owner: String,
    },

    #[error("'{set}' definiert '{name}' doppelt")]
    Duplicate { set: String, name: String },

    #[error("Kompositionszyklus: {0}")]
    CompositionCycle(String),

    #[error("Metadaten unvollstaendig: {0}")]
    Incomplete(String),
}

/// Spaltentyp. Bewusst ein geschlossenes Enum statt eines EDM-Strings, damit
/// jedes Backend eine vollstaendige Abbildung erzwungen bekommt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    Guid,
    Bool,
    Int32,
    Int64,
    Decimal { precision: u32, scale: u32 },
    String { max_length: Option<u32> },
    Date,
    DateTimeOffset,
    Binary,
}

impl DataType {
    /// Grobe Klassifikation fuer Sortierung und Vergleiche.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            DataType::Int32 | DataType::Int64 | DataType::Decimal { .. }
        )
    }
}

/// Rolle einer Spalte im Datenmodell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColumnRole {
    /// Normales Nutzdatenfeld.
    Data,
    /// Teil des Primaerschluessels.
    Key,
    /// Fremdschluessel auf `target_set`.
    ForeignKey { target_set: String },
    /// Einheiten-/Waehrungsfeld zu einer Measure-Spalte.
    Unit { for_column: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WritePolicy {
    /// Client darf den Wert jederzeit setzen/ändern.
    Writable,
    /// Client setzt den Wert nur beim Anlegen, danach unveränderlich.
    WriteOnCreate,
    /// Server berechnet den Wert bei jeder Operation neu, Client schreibt nie.
    Computed,
    /// Server berechnet den Wert einmalig beim Anlegen, danach für niemanden mehr änderbar.
    ComputedOnCreate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub default: Option<Value>,
    pub write_policy: WritePolicy,
    pub role: ColumnRole,
}

impl Column {
    pub fn new(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            name: name.into(),
            data_type,
            nullable: true,
            default: None,
            write_policy: WritePolicy::Writable,
            role: ColumnRole::Data,
        }
    }

    pub fn key(name: impl Into<String>, data_type: DataType) -> Self {
        Self {
            nullable: false,
            role: ColumnRole::Key,
            ..Self::new(name, data_type)
        }
    }

    pub fn foreign_key(name: impl Into<String>, target_set: impl Into<String>) -> Self {
        Self {
            role: ColumnRole::ForeignKey {
                target_set: target_set.into(),
            },
            ..Self::new(name, DataType::Guid)
        }
    }

    pub fn required(mut self) -> Self {
        self.nullable = false;
        self
    }

    pub fn computed(mut self) -> Self {
        self.write_policy = WritePolicy::Computed;
        self
    }

    pub fn immutable(mut self) -> Self {
        self.write_policy = WritePolicy::WriteOnCreate;
        self
    }

    pub fn with_default(mut self, value: Value) -> Self {
        self.default = Some(value);
        self
    }

    pub fn is_key(&self) -> bool {
        matches!(self.role, ColumnRole::Key)
    }

    pub fn is_immutable_on_change(&self) -> bool {
        !(self.write_policy == WritePolicy::Writable)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cardinality {
    ToOne,
    ToMany,
}

/// Komposition (Parent besitzt das Kind) vs. lose Assoziation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ownership {
    Composition,
    Association,
}

/// Welche Seite die Fremdschluesselspalte traegt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ForeignKeySide {
    /// Die Spalte liegt im eigenen Set (typisch fuer to-one).
    Here { column: String },
    /// Die Spalte liegt im Ziel-Set (typisch fuer to-many).
    There { column: String },
}

impl ForeignKeySide {
    pub fn column(&self) -> &str {
        match self {
            ForeignKeySide::Here { column } | ForeignKeySide::There { column } => column,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Association {
    /// Name der Navigationseigenschaft, z.B. `_Items`.
    pub name: String,
    pub target_set: String,
    pub cardinality: Cardinality,
    pub ownership: Ownership,
    pub foreign_key: ForeignKeySide,
    /// Name der Gegen-Navigation im Ziel-Set, falls deklariert.
    pub reverse: Option<String>,
}

impl Association {
    /// Gehaltenes Kind-Set: der Parent bestimmt den Lebenszyklus.
    pub fn is_composition_child(&self) -> bool {
        debug!(
            "is composition child: {:?} {:?}",
            self.ownership, self.cardinality
        );
        self.ownership == Ownership::Composition && self.cardinality == Cardinality::ToMany
    }

    /// Gehaltender Parent: die n:1-Gegenrichtung einer Komposition.
    pub fn is_composition_parent(&self) -> bool {
        self.ownership == Ownership::Composition && self.cardinality == Cardinality::ToOne
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitySet {
    /// Speichername, z.B. `OrderSet`.
    pub name: String,
    /// Fachlicher Entity-Name, z.B. `Order`.
    pub entity_name: String,
    /// Nimmt das Set am Draft-Flow teil (Spalte `IsActiveEntity`)?
    pub draft_enabled: bool,
    columns: IndexMap<String, Column>,
    associations: IndexMap<String, Association>,
}

impl EntitySet {
    pub fn new(name: impl Into<String>, entity_name: impl Into<String>) -> Self {
        let mut columns = IndexMap::new();
        columns.insert(
            ID_COLUMN.to_string(),
            Column::key(ID_COLUMN, DataType::Guid),
        );
        Self {
            name: name.into(),
            entity_name: entity_name.into(),
            draft_enabled: false,
            columns,
            associations: IndexMap::new(),
        }
    }

    /// Aktiviert den Draft-Flow und ergaenzt die Schluesselspalte.
    pub fn drafts(mut self) -> Self {
        self.draft_enabled = true;
        self.columns.insert(
            IS_ACTIVE_COLUMN.to_string(),
            Column::key(IS_ACTIVE_COLUMN, DataType::Bool),
        );
        self.columns
            .move_index(self.columns.len() - 1, 1.min(self.columns.len() - 1));
        self
    }

    pub fn column(mut self, column: Column) -> Self {
        self.columns.insert(column.name.clone(), column);
        self
    }

    pub fn association(mut self, association: Association) -> Self {
        self.associations
            .insert(association.name.clone(), association);
        self
    }

    pub fn columns(&self) -> impl Iterator<Item = &Column> {
        self.columns.values()
    }

    pub fn get_column(&self, name: &str) -> Result<&Column, SchemaError> {
        self.columns
            .get(name)
            .ok_or_else(|| SchemaError::UnknownColumn {
                set: self.name.clone(),
                column: name.to_string(),
            })
    }

    pub fn has_column(&self, name: &str) -> bool {
        self.columns.contains_key(name)
    }

    pub fn associations(&self) -> impl Iterator<Item = &Association> {
        self.associations.values()
    }

    pub fn get_association(&self, name: &str) -> Result<&Association, SchemaError> {
        self.associations
            .get(name)
            .ok_or_else(|| SchemaError::UnknownAssociation {
                set: self.name.clone(),
                association: name.to_string(),
            })
    }

    /// Schluesselspalten in Reihenfolge: `ID`, bei Drafts zusaetzlich
    /// `IsActiveEntity`.
    pub fn key_columns(&self) -> Vec<&Column> {
        self.columns.values().filter(|c| c.is_key()).collect()
    }

    /// Alle gehaltenen Kind-Navigationen.
    pub fn composition_children(&self) -> impl Iterator<Item = &Association> {
        self.associations
            .values()
            .filter(|a| a.is_composition_child())
    }

    /// Die haltende Parent-Navigation, falls das Set ein Kompositionskind ist.
    pub fn composition_parent(&self) -> Option<&Association> {
        self.associations
            .values()
            .find(|a| a.is_composition_parent())
    }
}

/// Validiertes Gesamtschema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Schema {
    sets: IndexMap<String, EntitySet>,
}

impl Schema {
    pub fn builder() -> SchemaBuilder {
        SchemaBuilder::default()
    }

    pub fn set(&self, set_name: &str) -> Result<&EntitySet, SchemaError> {
        self.sets
            .get(set_name)
            .ok_or_else(|| SchemaError::UnknownSet(set_name.to_string()))
    }

    pub fn contains(&self, set_name: &str) -> bool {
        self.sets.contains_key(set_name)
    }

    pub fn sets(&self) -> impl Iterator<Item = &EntitySet> {
        self.sets.values()
    }

    pub fn is_empty(&self) -> bool {
        self.sets.is_empty()
    }

    /// Sets topologisch nach Kompositionstiefe: Parents vor Kindern.
    /// Reihenfolge fuer DDL (Fremdschluessel) und Kaskaden.
    pub fn creation_order(&self) -> Result<Vec<&EntitySet>, SchemaError> {
        let mut depth: HashMap<&str, usize> = HashMap::new();
        for set in self.sets.values() {
            self.composition_depth(set, &mut depth, &mut Vec::new())?;
        }
        let mut sets: Vec<&EntitySet> = self.sets.values().collect();
        sets.sort_by_key(|s| (depth.get(s.name.as_str()).copied().unwrap_or(0), &s.name));
        Ok(sets)
    }

    fn composition_depth<'a>(
        &'a self,
        set: &'a EntitySet,
        depth: &mut HashMap<&'a str, usize>,
        stack: &mut Vec<String>,
    ) -> Result<usize, SchemaError> {
        if let Some(d) = depth.get(set.name.as_str()) {
            return Ok(*d);
        }
        if stack.contains(&set.name) {
            stack.push(set.name.clone());
            return Err(SchemaError::CompositionCycle(stack.join(" -> ")));
        }
        stack.push(set.name.clone());
        let d = match set.composition_parent() {
            Some(parent) => {
                let parent_set = self.set(&parent.target_set)?;
                self.composition_depth(parent_set, depth, stack)? + 1
            }
            None => 0,
        };
        stack.pop();
        depth.insert(&set.name, d);
        Ok(d)
    }

    /// Uebersetzt das Schema in die flache Navigationsliste, die die
    /// Dokumenten-Backends erwarten. Uebergangs-Shim, damit `Schema` schon
    /// genutzt werden kann, bevor alle Stores umgestellt sind.
    pub fn to_navigations(&self) -> Vec<crate::Navigation> {
        let mut navigations = vec![];
        for set in self.sets() {
            if set.draft_enabled {
                let mut nav = crate::Navigation::new(true);
                nav.from(&set.name, crate::DRAFT_ADMIN_SET)
                    .to(crate::DRAFT_ADMIN_SET, ID_COLUMN);
                navigations.push(nav);
            }
            for assoc in set.associations() {
                // Nur die to-many-Richtung emittieren: die Gegenrichtung wird
                // von `nav_links` erzeugt, sonst entstehen Duplikate.
                if assoc.cardinality == Cardinality::ToOne {
                    continue;
                }
                let mut nav = crate::Navigation::new(false);
                nav.owning = assoc.ownership == Ownership::Composition;
                nav.from(&set.name, &assoc.name)
                    .to(&assoc.target_set, assoc.foreign_key.column());
                navigations.push(nav);
            }
            // Skalare Navigationen ergaenzen, damit `$expand` sie findet.
            //
            // Sie tragen bewusst **keine** Ownership: in der flachen
            // Navigationsliste wird die Komposition allein von der
            // to-many-Richtung beschrieben, aus der `nav_links` die
            // Gegenrichtung samt Fremdschluesselspalte ableitet. Wuerde die
            // skalare Richtung ebenfalls als owning gelten, gaebe es zwei
            // Parent-Kandidaten - einen mit dem Navigationsnamen (`entity`)
            // und einen mit der FK-Spalte (`entity_ID`) - und nur letzterer
            // steht wirklich im Datensatz.
            for assoc in set.associations() {
                if assoc.cardinality != Cardinality::ToOne || assoc.reverse.is_some() {
                    continue;
                }
                let mut nav = crate::Navigation::new(true);
                nav.from(&set.name, &assoc.name)
                    .to(&assoc.target_set, ID_COLUMN);
                navigations.push(nav);
            }
        }
        navigations
    }

    pub fn save(&self, name: impl AsRef<std::path::Path>) -> anyhow::Result<()> {
        let writer = File::create(name)?;
        serde_json::to_writer(writer, &self)?;
        Ok(())
    }

    pub fn load(name: impl AsRef<std::path::Path>) -> anyhow::Result<Schema> {
        let reader = File::open(name)?;
        Ok(serde_json::from_reader::<_, Schema>(reader)?)
    }
}

/// Sammelt Sets ein und validiert sie beim Bauen.
#[derive(Debug, Default)]
pub struct SchemaBuilder {
    sets: IndexMap<String, EntitySet>,
}

impl SchemaBuilder {
    pub fn set(mut self, set: EntitySet) -> Self {
        self.sets.insert(set.name.clone(), set);
        self
    }

    pub fn add(&mut self, set: EntitySet) -> &mut Self {
        self.sets.insert(set.name.clone(), set);
        self
    }

    /// Validiert Referenzen und Fremdschluessel. Fehler statt stiller
    /// Auslassungen - genau das unterscheidet `Schema` von `Vec<Navigation>`.
    pub fn build(self) -> Result<Schema, SchemaError> {
        let schema = Schema { sets: self.sets };
        for set in schema.sets.values() {
            let mut seen = HashSet::new();
            for column in set.columns() {
                if !seen.insert(column.name.as_str()) {
                    return Err(SchemaError::Duplicate {
                        set: set.name.clone(),
                        name: column.name.clone(),
                    });
                }
                if let ColumnRole::ForeignKey { target_set } = &column.role {
                    if !schema.contains(target_set) && target_set != crate::DRAFT_ADMIN_SET {
                        return Err(SchemaError::DanglingTarget {
                            set: set.name.clone(),
                            association: column.name.clone(),
                            target: target_set.clone(),
                        });
                    }
                }
            }
            if set.key_columns().is_empty() {
                return Err(SchemaError::Incomplete(format!(
                    "{} hat keine Schluesselspalte",
                    set.name
                )));
            }

            for assoc in set.associations() {
                if !seen.insert(assoc.name.as_str()) {
                    return Err(SchemaError::Duplicate {
                        set: set.name.clone(),
                        name: assoc.name.clone(),
                    });
                }
                if assoc.target_set == crate::DRAFT_ADMIN_SET {
                    continue;
                }
                let target = schema.sets.get(&assoc.target_set).ok_or_else(|| {
                    SchemaError::DanglingTarget {
                        set: set.name.clone(),
                        association: assoc.name.clone(),
                        target: assoc.target_set.clone(),
                    }
                })?;
                // Die Fremdschluesselspalte muss dort existieren, wo sie laut
                // Deklaration liegt.
                let (owner, column) = match &assoc.foreign_key {
                    ForeignKeySide::Here { column } => (set, column),
                    ForeignKeySide::There { column } => (target, column),
                };
                if !owner.has_column(column) {
                    return Err(SchemaError::MissingForeignKey {
                        set: set.name.clone(),
                        association: assoc.name.clone(),
                        column: column.clone(),
                        owner: owner.name.clone(),
                    });
                }
            }
        }
        // Zyklen frueh melden.
        schema.creation_order()?;
        Ok(schema)
    }
}

/// Billiger Fingerabdruck des Schemas fuer Aenderungserkennung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SchemaRevision(pub u64);

/// Quelle der Metadaten. Der relationale Store haelt eine solche Quelle als
/// Konstruktor-Abhaengigkeit - Metadaten fliessen damit **nicht** mehr durch
/// den `DataStore`-Trait.
pub trait SchemaSource: Send + Sync {
    fn schema(&self) -> Result<Arc<Schema>, SchemaError>;

    /// Wechselt genau dann, wenn sich `schema()` aendert.
    fn revision(&self) -> SchemaRevision;
}

/// Unveraenderliche Quelle - fuer Tests und fuer fest eincompilierte
/// Bootstrap-Schemata.
pub struct StaticSchema {
    schema: Arc<Schema>,
    revision: SchemaRevision,
}

impl StaticSchema {
    pub fn new(schema: Schema) -> Self {
        Self {
            schema: Arc::new(schema),
            revision: SchemaRevision(1),
        }
    }

    pub fn with_revision(schema: Schema, revision: u64) -> Self {
        Self {
            schema: Arc::new(schema),
            revision: SchemaRevision(revision),
        }
    }
}

impl SchemaSource for StaticSchema {
    fn schema(&self) -> Result<Arc<Schema>, SchemaError> {
        Ok(self.schema.clone())
    }

    fn revision(&self) -> SchemaRevision {
        self.revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn orders_schema() -> Result<Schema, SchemaError> {
        Schema::builder()
            .set(
                EntitySet::new("OrderSet", "Order")
                    .drafts()
                    .column(Column::new(
                        "Status",
                        DataType::String {
                            max_length: Some(20),
                        },
                    ))
                    .column(Column::foreign_key("Customer_ID", "CustomerSet"))
                    .association(Association {
                        name: "_Items".into(),
                        target_set: "ItemSet".into(),
                        cardinality: Cardinality::ToMany,
                        ownership: Ownership::Composition,
                        foreign_key: ForeignKeySide::There {
                            column: "Order_ID".into(),
                        },
                        reverse: Some("Order".into()),
                    }),
            )
            .set(
                EntitySet::new("ItemSet", "Item")
                    .drafts()
                    .column(Column::foreign_key("Order_ID", "OrderSet"))
                    .association(Association {
                        name: "Order".into(),
                        target_set: "OrderSet".into(),
                        cardinality: Cardinality::ToOne,
                        ownership: Ownership::Composition,
                        foreign_key: ForeignKeySide::Here {
                            column: "Order_ID".into(),
                        },
                        reverse: Some("_Items".into()),
                    }),
            )
            .set(EntitySet::new("CustomerSet", "Customer").drafts())
            .build()
    }

    #[test]
    fn drafts_add_the_activation_key() {
        let schema = orders_schema().unwrap();
        let keys: Vec<_> = schema
            .set("OrderSet")
            .unwrap()
            .key_columns()
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(keys, vec![ID_COLUMN, IS_ACTIVE_COLUMN]);
    }

    #[test]
    fn creation_order_puts_parents_first() {
        let schema = orders_schema().unwrap();
        let order: Vec<_> = schema
            .creation_order()
            .unwrap()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        let parent = order.iter().position(|n| n == "OrderSet").unwrap();
        let child = order.iter().position(|n| n == "ItemSet").unwrap();
        assert!(parent < child, "{order:?}");
    }

    #[test]
    fn composition_helpers_agree_on_direction() {
        let schema = orders_schema().unwrap();
        let orders = schema.set("OrderSet").unwrap();
        let items = schema.set("ItemSet").unwrap();
        assert_eq!(orders.composition_children().count(), 1);
        assert!(orders.composition_parent().is_none());
        assert_eq!(items.composition_children().count(), 0);
        assert_eq!(items.composition_parent().unwrap().target_set, "OrderSet");
    }

    #[test]
    fn unknown_set_is_an_error_not_none() {
        let schema = orders_schema().unwrap();
        assert!(matches!(
            schema.set("Nope"),
            Err(SchemaError::UnknownSet(_))
        ));
    }

    #[test]
    fn dangling_association_target_is_rejected() {
        let result = Schema::builder()
            .set(
                EntitySet::new("OrderSet", "Order").association(Association {
                    name: "_Items".into(),
                    target_set: "Missing".into(),
                    cardinality: Cardinality::ToMany,
                    ownership: Ownership::Composition,
                    foreign_key: ForeignKeySide::There {
                        column: "Order_ID".into(),
                    },
                    reverse: None,
                }),
            )
            .build();
        assert!(matches!(result, Err(SchemaError::DanglingTarget { .. })));
    }

    #[test]
    fn missing_foreign_key_column_is_rejected() {
        let result = Schema::builder()
            .set(
                EntitySet::new("OrderSet", "Order").association(Association {
                    name: "_Items".into(),
                    target_set: "ItemSet".into(),
                    cardinality: Cardinality::ToMany,
                    ownership: Ownership::Composition,
                    foreign_key: ForeignKeySide::There {
                        column: "Order_ID".into(),
                    },
                    reverse: None,
                }),
            )
            .set(EntitySet::new("ItemSet", "Item")) // ohne Order_ID
            .build();
        assert!(matches!(result, Err(SchemaError::MissingForeignKey { .. })));
    }

    #[test]
    fn composition_cycles_are_rejected() {
        let assoc = |target: &str, name: &str| Association {
            name: name.into(),
            target_set: target.into(),
            cardinality: Cardinality::ToOne,
            ownership: Ownership::Composition,
            foreign_key: ForeignKeySide::Here {
                column: format!("{name}_ID"),
            },
            reverse: None,
        };
        let result = Schema::builder()
            .set(
                EntitySet::new("A", "A")
                    .column(Column::foreign_key("b_ID", "B"))
                    .association(assoc("B", "b")),
            )
            .set(
                EntitySet::new("B", "B")
                    .column(Column::foreign_key("a_ID", "A"))
                    .association(assoc("A", "a")),
            )
            .build();
        assert!(matches!(result, Err(SchemaError::CompositionCycle(_))));
    }

    /// Der Shim muss die Ownership tragen - genau die ging im alten
    /// `navigations_from_ddic` verloren (immer `false`).
    #[test]
    fn navigation_shim_preserves_ownership() {
        let schema = orders_schema().unwrap();
        let navs = schema.to_navigations();
        let items = navs
            .iter()
            .find(|n| n.from_field.as_deref() == Some("_Items"))
            .expect("_Items");
        assert!(items.owning, "Komposition muss als owning ankommen");
        assert!(!items.single);
        assert_eq!(items.to_set.as_deref(), Some("ItemSet"));
        assert_eq!(items.to_field.as_deref(), Some("Order_ID"));
    }

    #[test]
    fn navigation_shim_emits_draft_admin_link() {
        let schema = orders_schema().unwrap();
        let navs = schema.to_navigations();
        assert!(navs.iter().any(|n| {
            n.from_set.as_deref() == Some("OrderSet")
                && n.from_field.as_deref() == Some(crate::DRAFT_ADMIN_SET)
                && n.single
        }));
    }

    #[test]
    fn static_source_round_trips() {
        let source = StaticSchema::new(orders_schema().unwrap());
        assert_eq!(source.revision(), SchemaRevision(1));
        assert!(source.schema().unwrap().contains("OrderSet"));
    }
}
