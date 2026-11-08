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
    Association, Cardinality, Column, DataType, EntitySet, ForeignKeySide, LegacyValueColumn,
    Ownership, Schema, SchemaBuilder, SchemaError, SchemaRevision, SchemaSource, ID_SUFFIX,
};
use spec::data_dictionary::DataDictionary;
use spec::{EntitySpec, FieldSpec};
use tracing::warn;

/// Uebersetzt einen aktivierten DDIC-Stand in ein validiertes [`Schema`].
///
/// `ddic.activate()` muss vorher gelaufen sein, sonst sind keine
/// Lookup-Felder vorhanden.
pub fn schema_from_ddic(ddic: &DataDictionary) -> Result<Schema, SchemaError> {
    let entities: Vec<EntitySpec> = ddic.entities();
    // Kompositionen aus den Relationships zurueckgewinnen: (Entity, Navigationsname) -> owned
    let ownership = composition_index(ddic);

    let mut builder = SchemaBuilder::default();
    for entity in entities.iter() {
        let mut set = entity_set(ddic, entity, &ownership)?;
        for conv in ddic
            .converted_value_fields()
            .iter()
            .filter(|c| c.entity == entity.name())
        {
            let Some(list) = ddic.value_lists().iter().find(|l| l.id == conv.list_id) else {
                continue;
            };
            set.legacy_value_columns.push(LegacyValueColumn {
                old_column: conv.field.clone(),
                fk_column: format!("{}{ID_SUFFIX}", conv.field),
                mapping: list
                    .values
                    .iter()
                    .map(|v| {
                        (
                            v.code.clone(),
                            spec::value_list::entry_id(&list.id, &v.code),
                        )
                    })
                    .collect(),
            });
        }
        builder.add(set);
    }
    builder.build()
}

/// `(entity_name, navigation_name) -> Ownership`
fn composition_index(ddic: &DataDictionary) -> HashMap<(String, String), Ownership> {
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
    ddic: &DataDictionary,
    entity: &EntitySpec,
    ownership: &HashMap<(String, String), Ownership>,
) -> Result<EntitySet, SchemaError> {
    let mut set =
        EntitySet::new(entity.setname(), entity.name()).with_key_type(key_data_type(entity));
    if entity.draft_enabled {
        set = set.drafts();
    }

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
                // optional. Bewusste Festlegung, nicht Ableitung. Der eigene
                // Primaerschluessel ist Pflicht.
                column.nullable = entity.primary_key.as_deref() != Some(name.as_str());
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
            let column = ddic.fk_column(entity.name(), &fk);
            set = set.association(Association {
                name: name.clone(),
                target_set,
                cardinality: Cardinality::ToMany,
                ownership,
                foreign_key: ForeignKeySide::There { column },
                reverse: Some(fk),
            });
        } else {
            // to-one: der Fremdschluessel liegt im eigenen Set.
            let column = ddic.fk_column(target.as_str(), name);
            if !set.has_column(&column) {
                let mut fk = Column::foreign_key_typed(
                    &column,
                    &target_set,
                    key_type_of(ddic, target.as_str()),
                );
                // Verweise auf Benutzer zeigen im Zweifel auf den Default User; das
                // ist zugleich der DEFAULT fuer Spalten, die nachtraeglich entstehen.
                if target.as_str() == spec::system_user::SYSTEM_USER {
                    fk.default = Some(serde_json::json!(
                        spec::system_user::default_user_id().to_string()
                    ));
                }
                set = set.column(fk);
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
/// Datentyp des Primaerschluessels einer Entitaet (`Guid` ohne eigenen Schluessel).
fn key_data_type(entity: &EntitySpec) -> DataType {
    let Some(pk) = entity.primary_key.as_deref() else {
        return DataType::Guid;
    };
    entity
        .fields()
        .find_map(|f| match f {
            FieldSpec::Atom {
                name,
                edm_type,
                max_length,
                precision,
                scale,
                ..
            } if name == pk => Some(atom_data_type(edm_type, *max_length, *precision, *scale)),
            _ => None,
        })
        .unwrap_or(DataType::Guid)
}

/// Datentyp des Schluessels einer Ziel-Entitaet (fuer Fremdschluessel).
fn key_type_of(ddic: &DataDictionary, entity_name: &str) -> DataType {
    ddic.active_entity(entity_name)
        .map(key_data_type)
        .unwrap_or(DataType::Guid)
}

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
    pub fn new(ddic: &DataDictionary) -> Result<Self, SchemaError> {
        Ok(Self {
            inner: RwLock::new(Inner {
                schema: Arc::new(schema_from_ddic(ddic)?),
                revision: SchemaRevision(1),
            }),
        })
    }

    /// Uebernimmt einen neu geladenen DDIC-Stand.
    pub fn set_dictionary(&self, ddic: &DataDictionary) -> Result<SchemaRevision, SchemaError> {
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
mod tests;
