//! Adapter vom Data Dictionary (`spec`) auf das Speicher-Schema
//! (`data_store::schema`).
//!
//! Hier - und nur hier - werden die Luecken des DDIC geschlossen. `spec`
//! modelliert weder Nullability noch Defaults noch einen expliziten
//! Schluessel, und `DataDictonary::activate()` setzt `owned` in den
//! abgeleiteten Lookups hart auf `false` (data_dictionary.rs:365,373). Die
//! Komposition wird deshalb aus den `Relationship`s zurueckgewonnen.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use data_store::schema::{
    Association, Cardinality, Column, DataType, EntitySet, ForeignKeySide, Ownership, Schema,
    SchemaBuilder, SchemaError, SchemaRevision, SchemaSource, ID_SUFFIX,
};
use spec::data_dictionary::DataDictonary;
use spec::{EntitySpec, FieldSpec};
use tracing::warn;

/// Uebersetzt einen aktivierten DDIC-Stand in ein validiertes [`Schema`].
///
/// `ddic.activate()` muss vorher gelaufen sein, sonst sind keine
/// Lookup-Felder vorhanden.
pub fn schema_from_ddic(ddic: &DataDictonary) -> Result<Schema, SchemaError> {
    let entities: Vec<EntitySpec> = ddic.entities();
    // Kompositionen aus den Relationships zurueckgewinnen: (Entity, Navigationsname) -> owned
    let ownership = composition_index(ddic);

    let mut builder = SchemaBuilder::default();
    for entity in entities.iter() {
        builder.add(entity_set(ddic, entity, &ownership)?);
    }
    builder.build()
}

/// `(entity_name, navigation_name) -> Ownership`
fn composition_index(ddic: &DataDictonary) -> HashMap<(String, String), Ownership> {
    let mut index = HashMap::new();
    for rel in ddic.relationships() {
        let (Some(one), Some(one_field), Some(many), Some(many_property)) = (
            rel.one.as_ref(),
            rel.one_field.as_ref(),
            rel.many.as_ref(),
            rel.many_property.as_ref(),
        ) else {
            warn!("unvollstaendige Relationship '{}' ignoriert", rel.name);
            continue;
        };
        let ownership = if rel.is_owned() {
            Ownership::Composition
        } else {
            Ownership::Association
        };
        index.insert((one.clone(), one_field.clone()), ownership);
        index.insert((many.clone(), many_property.clone()), ownership);
    }
    index
}

fn entity_set(
    ddic: &DataDictonary,
    entity: &EntitySpec,
    ownership: &HashMap<(String, String), Ownership>,
) -> Result<EntitySet, SchemaError> {
    // Alle DDIC-Entitaeten nehmen am Draft-Flow teil.
    let mut set = EntitySet::new(entity.setname(), entity.name()).drafts();

    for field in entity.fields() {
        match field {
            FieldSpec::Atom {
                name,
                edm_type,
                max_length,
                precision,
                scale,
                computed,
                immutable,
                ..
            } => {
                let mut column = Column::new(
                    name,
                    atom_data_type(edm_type, *max_length, *precision, *scale),
                );
                // spec kennt keine Nullability: alles ausser dem Schluessel ist
                // optional. Bewusste Festlegung, nicht Ableitung.
                column.nullable = true;
                if *computed {
                    column = column.computed();
                }
                if *immutable {
                    column = column.immutable();
                }
                set = set.column(column);
            }
            FieldSpec::Measure {
                name,
                precision,
                scale,
                unit_field,
                ..
            } => {
                set = set.column(Column::new(
                    name,
                    DataType::Decimal {
                        precision: precision.unwrap_or(15),
                        scale: scale.unwrap_or(3),
                    },
                ));
                // Das Einheitenfeld ist ein eigener Atom im DDIC; falls es dort
                // fehlt, hier ergaenzen, damit die Spalte existiert.
                if !set.has_column(unit_field) {
                    set = set.column(Column::new(
                        unit_field,
                        DataType::String {
                            max_length: Some(5),
                        },
                    ));
                }
            }
            FieldSpec::Lookup { .. } => {} // Navigationen: unten
        }
    }

    for field in entity.fields() {
        let FieldSpec::Lookup {
            name,
            entity: target,
            collection,
            ..
        } = field
        else {
            continue;
        };
        let target_set = format!("{target}Set");
        let ownership = ownership
            .get(&(entity.name().to_string(), name.clone()))
            .copied()
            .unwrap_or(Ownership::Association);

        if *collection {
            // to-many: der Fremdschluessel liegt im Ziel-Set und heisst
            // `{many_property}_ID`.
            let Some(fk) = ddic.foreign_key_field(entity, field) else {
                warn!(
                    "kein Fremdschluessel fuer '{}' von '{}' - Navigation uebersprungen",
                    name,
                    entity.name()
                );
                continue;
            };
            set = set.association(Association {
                name: name.clone(),
                target_set,
                cardinality: Cardinality::ToMany,
                ownership,
                foreign_key: ForeignKeySide::There {
                    column: format!("{fk}{ID_SUFFIX}"),
                },
                reverse: Some(fk),
            });
        } else {
            // to-one: der Fremdschluessel liegt im eigenen Set.
            let column = format!("{name}{ID_SUFFIX}");
            if !set.has_column(&column) {
                set = set.column(Column::foreign_key(&column, &target_set));
            }
            set = set.association(Association {
                name: name.clone(),
                target_set,
                cardinality: Cardinality::ToOne,
                ownership,
                foreign_key: ForeignKeySide::Here { column },
                reverse: None,
            });
        }
    }

    Ok(set)
}

/// EDM-Typ auf [`DataType`]. Unbekannte Typen werden als String behandelt und
/// gemeldet - lieber eine breite Spalte als ein Abbruch beim Start.
fn atom_data_type(
    edm_type: &str,
    max_length: Option<u32>,
    precision: Option<u32>,
    scale: Option<u32>,
) -> DataType {
    match edm_type {
        "Edm.Guid" => DataType::Guid,
        "Edm.Boolean" => DataType::Bool,
        "Edm.Int16" | "Edm.Int32" | "Edm.Byte" | "Edm.SByte" => DataType::Int32,
        "Edm.Int64" => DataType::Int64,
        "Edm.Decimal" | "Edm.Double" | "Edm.Single" => DataType::Decimal {
            precision: precision.unwrap_or(15),
            scale: scale.unwrap_or(3),
        },
        "Edm.Date" => DataType::Date,
        "Edm.DateTimeOffset" | "Edm.DateTime" => DataType::DateTimeOffset,
        "Edm.Binary" => DataType::Binary,
        "Edm.String" => DataType::String { max_length },
        other => {
            warn!("unbekannter EDM-Typ '{other}' - als Edm.String behandelt");
            DataType::String { max_length }
        }
    }
}

/// [`SchemaSource`] ueber einem DDIC-Stand.
///
/// Das DDIC ist zur Laufzeit nachladbar; `set_dictionary` ersetzt den Stand
/// und zaehlt die Revision hoch, woran der relationale Store eine noetige
/// Migration erkennt.
pub struct DdicSchemaSource {
    inner: RwLock<Inner>,
}

struct Inner {
    schema: Arc<Schema>,
    revision: SchemaRevision,
}

impl DdicSchemaSource {
    pub fn new(ddic: &DataDictonary) -> Result<Self, SchemaError> {
        Ok(Self {
            inner: RwLock::new(Inner {
                schema: Arc::new(schema_from_ddic(ddic)?),
                revision: SchemaRevision(1),
            }),
        })
    }

    /// Uebernimmt einen neu geladenen DDIC-Stand.
    pub fn set_dictionary(&self, ddic: &DataDictonary) -> Result<SchemaRevision, SchemaError> {
        let schema = Arc::new(schema_from_ddic(ddic)?);
        let mut inner = self.inner.write().expect("schema lock");
        inner.schema = schema;
        inner.revision = SchemaRevision(inner.revision.0 + 1);
        Ok(inner.revision)
    }
}

impl SchemaSource for DdicSchemaSource {
    fn schema(&self) -> Result<Arc<Schema>, SchemaError> {
        Ok(self.inner.read().expect("schema lock").schema.clone())
    }

    fn revision(&self) -> SchemaRevision {
        self.inner.read().expect("schema lock").revision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_store::schema::ColumnRole;

    fn activated_ddic() -> DataDictonary {
        let mut ddic = DataDictonary::new();
        ddic.activate();
        ddic
    }

    #[test]
    fn bootstrap_dictionary_yields_a_valid_schema() {
        let schema = schema_from_ddic(&activated_ddic()).expect("schema");
        for expected in [
            "EntitySet",
            "FieldSet",
            "RelationSet",
            "PackageSet",
            "AppSet",
        ] {
            assert!(schema.contains(expected), "{expected} fehlt");
        }
    }

    #[test]
    fn every_set_has_the_draft_key() {
        let schema = schema_from_ddic(&activated_ddic()).unwrap();
        for set in schema.sets() {
            let keys: Vec<_> = set.key_columns().iter().map(|c| c.name.clone()).collect();
            assert_eq!(keys, vec!["ID", "IsActiveEntity"], "{}", set.name);
        }
    }

    /// Regression gegen den DDIC-Defekt: `activate()` setzt `owned` in den
    /// Lookups hart auf `false`. Die Komposition muss aus den Relationships
    /// kommen, sonst ist jede Kaskade im Store wirkungslos.
    #[test]
    fn composition_is_recovered_from_relationships() {
        let schema = schema_from_ddic(&activated_ddic()).unwrap();
        let entity_set = schema.set("EntitySet").unwrap();
        let fields = entity_set
            .associations()
            .find(|a| a.name == "_Fields")
            .expect("_Fields");
        assert_eq!(
            fields.ownership,
            Ownership::Composition,
            "entity_fields ist als owned deklariert"
        );
        assert!(fields.is_composition_child());

        // Gegenprobe: entity_package ist keine Komposition.
        let package = entity_set
            .associations()
            .find(|a| a.name == "package")
            .expect("package");
        assert_eq!(package.ownership, Ownership::Association);
    }

    #[test]
    fn to_many_foreign_key_lands_on_the_child_with_id_suffix() {
        let schema = schema_from_ddic(&activated_ddic()).unwrap();
        let fields = schema
            .set("EntitySet")
            .unwrap()
            .associations()
            .find(|a| a.name == "_Fields")
            .unwrap()
            .clone();
        assert_eq!(fields.cardinality, Cardinality::ToMany);
        assert_eq!(fields.target_set, "FieldSet");
        assert_eq!(
            fields.foreign_key,
            ForeignKeySide::There {
                column: "entity_ID".into()
            }
        );
        // und die Spalte existiert im Kind wirklich
        let field_set = schema.set("FieldSet").unwrap();
        assert!(matches!(
            field_set.get_column("entity_ID").unwrap().role,
            ColumnRole::ForeignKey { .. }
        ));
    }

    #[test]
    fn to_one_creates_the_foreign_key_column() {
        let schema = schema_from_ddic(&activated_ddic()).unwrap();
        let field_set = schema.set("FieldSet").unwrap();
        let entity = field_set
            .associations()
            .find(|a| a.name == "entity")
            .unwrap();
        assert_eq!(entity.cardinality, Cardinality::ToOne);
        assert_eq!(
            entity.foreign_key,
            ForeignKeySide::Here {
                column: "entity_ID".into()
            }
        );
    }

    #[test]
    fn edm_types_map_to_column_types() {
        let schema = schema_from_ddic(&activated_ddic()).unwrap();
        let field_set = schema.set("FieldSet").unwrap();
        let name = field_set.get_column("name").unwrap();
        assert!(matches!(name.data_type, DataType::String { .. }));
    }

    #[test]
    fn revision_advances_on_reload() {
        let ddic = activated_ddic();
        let source = DdicSchemaSource::new(&ddic).unwrap();
        assert_eq!(source.revision(), SchemaRevision(1));
        source.set_dictionary(&ddic).unwrap();
        assert_eq!(source.revision(), SchemaRevision(2));
    }

    #[test]
    fn navigation_shim_from_ddic_marks_compositions() {
        let schema = schema_from_ddic(&activated_ddic()).unwrap();
        let navs = schema.to_navigations();
        let fields = navs
            .iter()
            .find(|n| {
                n.from_set.as_deref() == Some("EntitySet")
                    && n.from_field.as_deref() == Some("_Fields")
            })
            .expect("_Fields navigation");
        assert!(
            fields.owning,
            "die Ownership ging im alten navigations_from_ddic verloren"
        );
    }
}
