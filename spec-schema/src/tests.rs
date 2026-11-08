use super::*;
use data_store::schema::ColumnRole;

fn activated_ddic() -> DataDictionary {
    let mut ddic = DataDictionary::new();
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
            n.from_set.as_deref() == Some("EntitySet") && n.from_field.as_deref() == Some("_Fields")
        })
        .expect("_Fields navigation");
    assert!(
        fields.owning,
        "die Ownership ging im alten navigations_from_ddic verloren"
    );
}
