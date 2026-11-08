use super::*;

#[test]
fn activate_exposes_lookup_fields_with_cardinality() {
    let mut ddic = DataDictionary::new();
    ddic.activate();

    let package = ddic
        .active_entity_by_setname("PackageSet")
        .expect("active entity by set name");
    let package_fields = package.fields().collect::<Vec<_>>();

    assert!(package_fields.iter().any(|field| matches!(
        field,
        FieldSpec::Lookup { name, entity, collection, owned, .. }
            if name == "_Entities" && entity == "Entity" && *collection
    )));

    let entity = ddic
        .active_entity_by_setname("EntitySet")
        .expect("active entity by set name");
    let entity_fields = entity.fields().collect::<Vec<_>>();

    assert!(entity_fields.iter().any(|field| matches!(
        field,
        FieldSpec::Lookup { name, entity, collection, owned, .. }
            if name == "_Fields" && entity == "Field" && *collection
    )));
    assert!(entity_fields.iter().any(|field| matches!(
        field,
        FieldSpec::Lookup { name, entity, collection, owned, .. }
            if name == "_Many" && entity == "Relation" && *collection
    )));
    assert!(entity_fields.iter().any(|field| matches!(
        field,
        FieldSpec::Lookup { name, entity, collection, owned, .. }
            if name == "package" && entity == "Package" && !*collection
    )));

    let field = ddic
        .active_entity_by_setname("FieldSet")
        .expect("active field by set name");
    let field_fields = field.fields().collect::<Vec<_>>();

    assert!(field_fields.iter().any(|field| matches!(
        field,
        FieldSpec::Lookup { name, entity, collection, owned, .. }
            if name == "entity" && entity == "Entity" && !*collection
    )));

    let package_by_name = ddic
        .active_entity("Package")
        .expect("active entity by name");
    assert_eq!(package_by_name.setname(), "PackageSet");
}

#[test]
fn derives_foreign_key_field_from_active_entities() {
    let mut ddic = DataDictionary::new();
    ddic.activate();

    let entity = ddic.active_entity("Entity").expect("active entity");
    let fields_lookup = entity
        .fields()
        .find(|f| matches!(f, FieldSpec::Lookup { name, .. } if name == "_Fields"))
        .expect("_Fields lookup");
    assert_eq!(
        ddic.foreign_key_field(entity, fields_lookup).as_deref(),
        Some("entity"),
    );

    let field = ddic.active_entity("Field").expect("active field");
    let parent_lookup = field
        .fields()
        .find(|f| matches!(f, FieldSpec::Lookup { name, .. } if name == "entity"))
        .expect("entity lookup");
    assert_eq!(
        ddic.foreign_key_field(field, parent_lookup).as_deref(),
        Some("entity"),
    );
}
