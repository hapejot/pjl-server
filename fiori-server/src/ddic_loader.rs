//! Liest die im Storage gepflegten Metadaten und ergaenzt damit das
//! eincompilierte Data Dictionary. Lookups werden dabei nie gelesen - sie
//! entstehen ausschliesslich beim Aktivieren aus den Relationships.

use std::collections::{HashMap, HashSet};

use odata::query::ODataQuery;
use data_store::{DataRecord, DataStore, Navigation};
use serde_json::Value;
use spec::{
    EntitySpec, FieldSpec, Relationship, app::App, data_dictionary::DataDictonary, package::Package,
};
use tracing::{info, warn};

const DEFAULT_PACKAGE: &str = "core";

struct LoadedEntity {
    name: String,
    package: String,
}

pub fn load_ddic_from_store(ddic: &mut DataDictonary, store: &dyn DataStore) {
    let packages = load_packages(ddic, store);
    let entities = load_entities(ddic, store, &packages);
    let known: HashSet<String> = ddic.entity_names().map(str::to_string).collect();

    load_fields(ddic, store, &entities);
    load_relationships(ddic, store, &entities, &known);
    load_apps(ddic, store, &entities, &packages, &known);
}

/// Alle Navigationen, die der Store braucht um die Abhängigkeiten zwischen den Records zu kennen..
pub fn navigations_from_ddic(ddic: &DataDictonary) -> Vec<Navigation> {
    let mut navigations: Vec<Navigation> = Vec::new();
    for entity in ddic.entities().iter() {
        let from_set = format!("{}Set", entity.name());
        let mut draft_nav = Navigation::new(true);
        draft_nav
            .from(&from_set, "DraftAdministrativeData")
            .to("DraftAdministrativeData", "ID");
        navigations.push(draft_nav);
        for field in entity.fields() {
            let FieldSpec::Lookup {
                name,
                entity: target,
                collection,
                owned: _,
            } = field
            else {
                continue;
            };
            // Der Store filtert To-Many ueber die FK-Spalte des Kindes, To-One ueber dessen ID.
            let to_field = if *collection {
                match ddic.foreign_key_field(entity, field) {
                    Some(fk) => format!("{fk}{}", spec::ID_SUFFIX),
                    None => {
                        warn!("no foreign key for '{}' of '{}'", name, entity.name());
                        continue;
                    }
                }
            } else {
                "ID".to_string()
            };
            let mut navigation = Navigation::new(!collection);
            navigation
                .from(&from_set, name)
                .to(&format!("{target}Set"), &to_field);
            navigations.push(navigation);
        }
    }
    navigations
}

fn load_packages(ddic: &mut DataDictonary, store: &dyn DataStore) -> HashMap<String, String> {
    let mut by_id = HashMap::new();
    for record in read_set(store, "PackageSet") {
        let Some(name) = text(&record, "name") else {
            warn!("package record {} without name - skipped", record.id());
            continue;
        };
        by_id.insert(record.id().to_string(), name.clone());
        ddic.add_package(Package::new(&name));
    }
    by_id
}

fn load_entities(
    ddic: &mut DataDictonary,
    store: &dyn DataStore,
    packages: &HashMap<String, String>,
) -> HashMap<String, LoadedEntity> {
    let mut by_id = HashMap::new();
    for record in read_set(store, "EntitySet") {
        let Some(name) = text(&record, "name") else {
            warn!("entity record {} without name - skipped", record.id());
            continue;
        };
        let plural = text(&record, "plural").unwrap_or_else(|| format!("{name}s"));
        let package = package_of(&record, packages);
        if ddic.add_entity(EntitySpec::new(&name, &package, &plural)) {
            by_id.insert(record.id().to_string(), LoadedEntity { name, package });
        } else {
            warn!("entity '{name}' is already defined - storage record ignored");
        }
    }
    by_id
}

fn load_fields(
    ddic: &mut DataDictonary,
    store: &dyn DataStore,
    entities: &HashMap<String, LoadedEntity>,
) {
    for record in read_set(store, "FieldSet") {
        let Some(entity) = text(&record, "entity_ID").and_then(|id| entities.get(&id)) else {
            warn!(
                "field record {} without known entity - skipped",
                record.id()
            );
            continue;
        };
        let Some(field) = atom_field(&record) else {
            continue;
        };
        if !ddic.add_field(&entity.name, field) {
            warn!("field of record {} could not be added", record.id());
        }
    }
}

fn load_relationships(
    ddic: &mut DataDictonary,
    store: &dyn DataStore,
    entities: &HashMap<String, LoadedEntity>,
    known: &HashSet<String>,
) {
    for record in read_set(store, "RelationSet") {
        let source = entity_side(&record, "one_ID", "source", entities, known);
        let target = entity_side(&record, "many_ID", "target", entities, known);
        let (Some(source), Some(target)) = (source, target) else {
            warn!(
                "relation record {} does not reference known entities - skipped",
                record.id()
            );
            continue;
        };

        // Ohne verwertbare Kardinalitaet gilt: source ist die 1-Seite.
        let source_is_one = !is_many(text(&record, "source_cardinality"))
            || is_many(text(&record, "target_cardinality"));
        let (one, one_field, many, many_property) = if source_is_one {
            (
                source,
                text(&record, "source_field"),
                target,
                text(&record, "target_field"),
            )
        } else {
            (
                target,
                text(&record, "target_field"),
                source,
                text(&record, "source_field"),
            )
        };

        let name = text(&record, "name").unwrap_or_else(|| format!("{one}_{many}"));
        let package = entities
            .values()
            .find(|e| e.name == one)
            .map(|e| e.package.clone())
            .unwrap_or_else(|| DEFAULT_PACKAGE.to_string());
        let one_field = one_field.unwrap_or_else(|| format!("_{many}s"));
        let many_property = many_property.unwrap_or_else(|| one.to_lowercase());

        let mut relationship = Relationship::new(&name, package)
            .with_one(one, one_field)
            .with_many(many, many_property);
        if flag(&record, "owned") {
            relationship = relationship.owned();
        }
        if !ddic.add_relationship(relationship) {
            warn!("relationship '{name}' is already defined - storage record ignored");
        }
    }
}

fn load_apps(
    ddic: &mut DataDictonary,
    store: &dyn DataStore,
    entities: &HashMap<String, LoadedEntity>,
    packages: &HashMap<String, String>,
    known: &HashSet<String>,
) {
    for record in read_set(store, "AppSet") {
        let Some(name) = text(&record, "name") else {
            warn!("app record {} without name - skipped", record.id());
            continue;
        };
        // Ein unbekanntes Hauptentity laesst `App::main_entity` spaeter panicken.
        let Some(main_entity) =
            entity_side(&record, "main_entity_ID", "main_entity", entities, known)
        else {
            warn!("app '{name}' references an unknown entity - skipped");
            continue;
        };
        let title = text(&record, "title").unwrap_or_else(|| name.clone());
        let package = package_of(&record, packages);
        if !ddic.add_app(App::new(
            name.clone(),
            title,
            main_entity,
            text(&record, "subtitle"),
            text(&record, "icon"),
            Package::new(&package),
        )) {
            warn!("app '{name}' is already defined - storage record ignored");
        }
    }
}

fn read_set(store: &dyn DataStore, set: &str) -> Vec<DataRecord> {
    match store.get_collection(set, &ODataQuery::new()) {
        Ok(records) => {
            info!("loaded {} records from {set}", records.len());
            records
        }
        Err(e) => {
            warn!("cannot read {set}: {e}");
            Vec::new()
        }
    }
}

fn atom_field(record: &DataRecord) -> Option<FieldSpec> {
    let name = text(record, "name")?;
    if name.ends_with(spec::ID_SUFFIX) || is_relation_role(text(record, "role")) {
        return None;
    }
    let label = text(record, "label").unwrap_or_else(|| name.clone());
    let edm_type = text(record, "type").unwrap_or_else(|| "Edm.String".to_string());

    let mut field = FieldSpec::atom(&name, &label, &edm_type);
    if let FieldSpec::Atom {
        max_length,
        precision,
        scale,
        computed,
        immutable,
        ..
    } = &mut field
    {
        *max_length = number(record, "max_length");
        *precision = number(record, "precision");
        *scale = number(record, "scale");
        *computed = flag(record, "computed");
        *immutable = flag(record, "immutable");
    }
    Some(field)
}

/// Eine Beziehungsseite ueber den Lookup aufloesen, ersatzweise ueber den Entity-Namen.
fn entity_side(
    record: &DataRecord,
    id_field: &str,
    name_field: &str,
    entities: &HashMap<String, LoadedEntity>,
    known: &HashSet<String>,
) -> Option<String> {
    text(record, id_field)
        .and_then(|id| entities.get(&id).map(|e| e.name.clone()))
        .or_else(|| text(record, name_field).filter(|name| known.contains(name)))
}

fn package_of(record: &DataRecord, packages: &HashMap<String, String>) -> String {
    text(record, "package_ID")
        .and_then(|id| packages.get(&id).cloned())
        .unwrap_or_else(|| DEFAULT_PACKAGE.to_string())
}

fn is_relation_role(role: Option<String>) -> bool {
    matches!(
        role.as_deref(),
        Some("lookup" | "association" | "composition" | "navigation")
    )
}

fn is_many(cardinality: Option<String>) -> bool {
    matches!(
        cardinality.as_deref(),
        Some("N" | "n" | "*" | "many" | "0..*" | "1..*" | "0..n" | "1..n")
    )
}

fn text(record: &DataRecord, field: &str) -> Option<String> {
    match record.get(field) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s),
        _ => None,
    }
}

fn number(record: &DataRecord, field: &str) -> Option<u32> {
    record.get(field).and_then(|v| v.as_u64()).map(|n| n as u32)
}

fn flag(record: &DataRecord, field: &str) -> bool {
    record
        .get(field)
        .and_then(|v| v.as_bool())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use odata_yaml_store::YamlStore;
    use serde_json::json;
    use spec::FieldSpec;
    use tempfile::TempDir;

    const PACKAGE_ID: &str = "00000000-0000-0000-0000-0000000000a1";
    const ORDER_ID: &str = "00000000-0000-0000-0000-000000000001";
    const ITEM_ID: &str = "00000000-0000-0000-0000-000000000002";

    fn store_with_metadata() -> (TempDir, YamlStore) {
        let dir = TempDir::new().unwrap();
        let store = YamlStore::open(dir.path()).unwrap();
        store
            .create_entity(
                "PackageSet",
                &json!({ "ID": PACKAGE_ID, "IsActiveEntity": true, "name": "sales" }),
            )
            .unwrap();
        store
            .create_entity(
                "EntitySet",
                &json!({ "ID": ORDER_ID, "IsActiveEntity": true,
                         "name": "Order", "plural": "Orders", "package_ID": PACKAGE_ID }),
            )
            .unwrap();
        store
            .create_entity(
                "EntitySet",
                &json!({ "ID": ITEM_ID, "IsActiveEntity": true,
                         "name": "Item", "plural": "Items", "package_ID": PACKAGE_ID }),
            )
            .unwrap();
        store
            .create_entity(
                "FieldSet",
                &json!({ "IsActiveEntity": true, "entity_ID": ORDER_ID,
                         "name": "status", "label": "Status", "type": "Edm.String",
                         "max_length": 20, "immutable": true }),
            )
            .unwrap();
        (dir, store)
    }

    fn loaded(store: &YamlStore) -> DataDictonary {
        let mut ddic = DataDictonary::new();
        load_ddic_from_store(&mut ddic, store);
        ddic.activate();
        ddic
    }

    #[test]
    fn loads_packages_entities_and_fields() {
        let (_dir, store) = store_with_metadata();
        let ddic = loaded(&store);

        let order = ddic.active_entity_by_setname("OrderSet").unwrap();
        assert_eq!(order.name(), "Order");
        let status = order.fields().find(|f| f.name() == "status").unwrap();
        assert!(matches!(
            status,
            FieldSpec::Atom {
                max_length: Some(20),
                immutable: true,
                ..
            }
        ));
    }

    #[test]
    fn derives_lookups_from_a_stored_relation() {
        let (_dir, store) = store_with_metadata();
        store
            .create_entity(
                "RelationSet",
                &json!({ "IsActiveEntity": true, "name": "order_items",
                         "one_ID": ORDER_ID, "source_field": "_Items",
                         "many_ID": ITEM_ID, "target_field": "order" }),
            )
            .unwrap();
        let ddic = loaded(&store);

        let order = ddic.active_entity_by_setname("OrderSet").unwrap();
        let items = order.fields().find(|f| f.name() == "_Items").unwrap();
        assert!(matches!(
            items,
            FieldSpec::Lookup {
                entity,
                collection: true,
                ..
            } if entity == "Item"
        ));
        assert_eq!(
            ddic.foreign_key_field(order, items).as_deref(),
            Some("order")
        );

        let navigation = navigations_from_ddic(&ddic)
            .into_iter()
            .find(|n| n.from_field.as_deref() == Some("_Items"))
            .unwrap();
        assert_eq!(navigation.from_set.as_deref(), Some("OrderSet"));
        assert_eq!(navigation.to_set.as_deref(), Some("ItemSet"));
        assert_eq!(navigation.to_field.as_deref(), Some("order_ID"));
        assert!(!navigation.single);
    }

    #[test]
    fn resolves_relation_sides_by_entity_name() {
        let (_dir, store) = store_with_metadata();
        store
            .create_entity(
                "RelationSet",
                &json!({ "IsActiveEntity": true, "name": "by_name",
                         "source": "Order", "source_field": "_Positions",
                         "target": "Item", "target_field": "head" }),
            )
            .unwrap();
        store
            .create_entity(
                "RelationSet",
                &json!({ "IsActiveEntity": true, "name": "junk",
                         "source": "S", "target": "T" }),
            )
            .unwrap();
        let ddic = loaded(&store);

        assert!(ddic.relationships().any(|r| r.name() == "by_name"));
        assert!(!ddic.relationships().any(|r| r.name() == "junk"));
    }

    #[test]
    fn loads_apps_and_skips_unknown_main_entity() {
        let (_dir, store) = store_with_metadata();
        store
            .create_entity(
                "AppSet",
                &json!({ "IsActiveEntity": true, "name": "orders", "title": "Orders",
                         "subtitle": "Sales orders", "icon": "sap-icon://cart",
                         "main_entity_ID": ORDER_ID, "package_ID": PACKAGE_ID }),
            )
            .unwrap();
        store
            .create_entity(
                "AppSet",
                &json!({ "IsActiveEntity": true, "name": "broken", "main_entity": "Nope" }),
            )
            .unwrap();
        let ddic = loaded(&store);

        let app = ddic.apps().find(|a| a.name() == "orders").unwrap();
        assert_eq!(app.main_entity(&ddic).name(), "Order");
        assert_eq!(app.subtitle().map(String::as_str), Some("Sales orders"));
        assert_eq!(app.icon().map(String::as_str), Some("sap-icon://cart"));
        assert!(!ddic.apps().any(|a| a.name() == "broken"));
    }
}
