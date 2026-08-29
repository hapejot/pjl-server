use std::collections::HashMap;

use crate::{EntitySpec, FieldSpec, Relationship, app::App, package::Package};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationInfo {
    pub name: String,
    pub source_entity: String,
    pub source_set: String,
    pub target_entity: String,
    pub target_set: String,
    pub collection: bool,
    pub foreign_key_field: Option<String>,
}

pub struct DataDictonary {
    packages: Vec<Package>,
    entities: Vec<EntitySpec>,
    relationships: Vec<Relationship>,
    apps: Vec<App>,
    active: HashMap<String, EntitySpec>,
}

impl DataDictonary {
    pub fn new() -> Self {
        let _core = Package::new("core");
        let core = Package::new("core");

        let mut package_fld = FieldSpec::atom("package", "Package", "Edm.Guid");
        package_fld.lookup("Package");

        let entity_ent = EntitySpec::new("Entity", core.name(), "Entities")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            // .field(FieldSpec::atom("package", "Package", "Edm.String"))
            // .field(package_fld)
            .field(FieldSpec::atom("plural", "Plural", "Edm.String"));

        let field_ent = EntitySpec::new("Field", core.name(), "Fields")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("role", "Role", "Edm.String"))
            .field(FieldSpec::atom("label", "Label", "Edm.String"))
            .field(FieldSpec::atom("type", "Type", "Edm.String"))
            .field(FieldSpec::atom("package", "Package", "Edm.String"))
            .field(FieldSpec::atom("max_length", "MaxLength", "Edm.Int"))
            .field(FieldSpec::atom("precision", "Precision", "Edm.Int"))
            .field(FieldSpec::atom("scale", "Scale", "Edm.Int"))
            .field(FieldSpec::atom("computed", "Package", "Edm.Boolean"))
            .field(FieldSpec::atom("immutable", "Immutable", "Edm.Boolean"))
            .field(FieldSpec::atom("package", "Package", "Edm.String"));

        let relation_ent = EntitySpec::new("Relation", core.name(), "Relations")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("source", "Source", "Edm.String"))
            .field(FieldSpec::atom(
                "source_field",
                "Source Field",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "source_cardinality",
                "Source Cardinality",
                "Edm.String",
            ))
            .field(FieldSpec::atom("target", "Target", "Edm.String"))
            .field(FieldSpec::atom(
                "target_field",
                "Target Field",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "target_cardinality",
                "Target Cardinality",
                "Edm.String",
            ));

        let value_ent = EntitySpec::new("Value", core.name(), "Values")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("value", "Value", "Edm.String"))
            .field(FieldSpec::atom("label", "Label", "Edm.String"));
        // .field(FieldSpec::atom("package", "Package", "Edm.Guid"));

        let package_ent = EntitySpec::new("Package", core.name(), "Packages")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("title", "Title", "Edm.String"))
            .field(FieldSpec::atom("description", "Description", "Edm.String"));

        let rel = Relationship::new("entity_fields", core.name())
            .with_one("Entity", "_Fields")
            .with_many("Field", "entity");
        let r2 = Relationship::new("entity_package", core.name())
            .with_many("Entity", "package")
            .with_one("Package", "_Entitites");
        let r3 = Relationship::new("many_entity", core.name())
            .with_one("Entity", "_Many")
            .with_many("Relation", "many");
        let r4 = Relationship::new("one_entity", core.name())
            .with_one("Entity", "_One")
            .with_many("Relation", "one");
        let r5 = Relationship::new("field_package", core.name())
            .with_many("Field", "package")
            .with_one("Package", "_Fields");
        let a1 = App::new(
            "relations".into(),
            "Relations".into(),
            "Relation".into(),
            Some("Maintain Relations".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a2 = App::new(
            "values".into(),
            "Values".into(),
            "Value".into(),
            Some("Maintain Values".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a3 = App::new(
            "entities".into(),
            "Entities".into(),
            "Entity".into(),
            Some("Maintain Entities".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );
        let a4 = App::new(
            "packages".into(),
            "Packages".into(),
            "Package".into(),
            Some("Maintain Packages".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );

        Self {
            packages: vec![core],
            entities: vec![entity_ent, relation_ent, field_ent, value_ent, package_ent],
            relationships: vec![rel, r2, r3, r4, r5],
            apps: vec![a1, a2, a3, a4],
            active: HashMap::new(),
        }
    }

    pub fn apps(&self) -> impl Iterator<Item = &App> {
        self.apps.iter()
    }

    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }

    pub fn entities(&self) -> Vec<EntitySpec> {
        let mut entities = self.entities.clone();
        for relation in self.relationships.iter() {
            let Relationship {
                one,
                one_field,
                many,
                many_property,
                ..
            } = relation;
            if let Some(e) = entities
                .iter_mut()
                .find(|x| &x.name == one.as_ref().unwrap())
            {
                e.fields.push(FieldSpec::Lookup {
                    name: one_field.clone().unwrap(),
                    entity: many.clone().unwrap(),
                    collection: true,
                });
            }

            if let Some(e) = entities
                .iter_mut()
                .find(|x| &x.name == many.as_ref().unwrap())
            {
                e.fields.push(FieldSpec::Lookup {
                    name: many_property.clone().unwrap(),
                    entity: one.clone().unwrap(),
                    collection: false,
                });
            }
        }
        entities
    }

    pub fn relationships(&self) -> impl Iterator<Item = &Relationship> {
        self.relationships.iter()
    }

    pub fn navigations_for_entity(&self, entity: &EntitySpec) -> Vec<NavigationInfo> {
        let entities = if self.active.is_empty() {
            self.entities()
        } else {
            self.active.values().cloned().collect()
        };

        entity
            .fields()
            .filter_map(|field| match field {
                FieldSpec::Lookup {
                    name,
                    entity: target_entity,
                    collection,
                } => {
                    let target = entities.iter().find(|candidate| candidate.name() == target_entity)?;
                    Some(NavigationInfo {
                        name: name.clone(),
                        source_entity: entity.name().to_string(),
                        source_set: entity.setname(),
                        target_entity: target_entity.clone(),
                        target_set: target.setname(),
                        collection: *collection,
                        foreign_key_field: self.foreign_key_field_for_navigation(
                            entity,
                            target,
                            name,
                            *collection,
                            &entities,
                        ),
                    })
                }
                _ => None,
            })
            .collect()
    }

    pub fn navigation_for_entity(
        &self,
        entity: &EntitySpec,
        navigation_property: &str,
    ) -> Option<NavigationInfo> {
        self.navigations_for_entity(entity)
            .into_iter()
            .find(|navigation| navigation.name == navigation_property)
    }

    fn foreign_key_field_for_navigation(
        &self,
        source: &EntitySpec,
        target: &EntitySpec,
        navigation_name: &str,
        collection: bool,
        entities: &[EntitySpec],
    ) -> Option<String> {
        if !collection {
            return Some(navigation_name.to_string());
        }

        self.relationships()
            .find(|relationship| {
                relationship.one() == source.name()
                    && relationship.many() == target.name()
                    && relationship.one_property() == navigation_name
            })
            .map(|relationship| relationship.many_property().to_string())
            .or_else(|| {
                entities
                    .iter()
                    .find(|candidate| candidate.name() == target.name())?
                    .fields()
                    .find_map(|field| match field {
                        FieldSpec::Lookup {
                            name,
                            entity,
                            collection,
                        } if entity == source.name() && !collection => Some(name.clone()),
                        _ => None,
                    })
            })
    }

    pub fn active_entity(&self, name: &str) -> Option<&EntitySpec> {
        self.active.get(name)
    }

    pub fn active_entities(&self) -> impl Iterator<Item = &EntitySpec> {
        self.active.values()
    }

    pub fn active_entity_by_setname(&self, set_name: &str) -> Option<&EntitySpec> {
        self.active.values().find(|entity| entity.setname() == set_name)
    }

    pub fn activate(&mut self) {
        for entitiy in self.entities.iter() {
            let mut active = entitiy.clone();

            for r in self.relationships() {
                if let Relationship {
                    one: Some(one),
                    one_field: Some(one_field),
                    many: Some(many),
                    many_property: Some(many_property),
                    ..
                } = &r
                {
                    if one == &active.name {
                        active.fields.push(FieldSpec::Lookup {
                            name: one_field.clone(),
                            entity: many.clone(),
                            collection: true,
                        });
                    }
                    if many == &active.name {
                        active.fields.push(FieldSpec::Lookup {
                            name: many_property.clone(),
                            entity: one.clone(),
                            collection: false,
                        });
                    }
                }
            }
            self.active.insert(active.name.clone(), active);
        }
    }
    pub fn dump(&self) {
        println!("==== Apps ===================================");
        for app in self.apps.iter() {
            println!("{:}", app.name());
        }
        println!("==== Packages ===============================");
        for package in self.packages() {
            println!("{:?}", package.name());
        }
        println!("==== Entities ===============================");
        for (_, entity) in self.active.iter() {
            println!("{}", entity.name);
            for prop in entity.fields.iter() {
                println!("    {}", prop.name());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_exposes_lookup_fields_with_cardinality() {
        let mut ddic = DataDictonary::new();
        ddic.activate();

        let package = ddic
            .active_entity_by_setname("PackageSet")
            .expect("active entity by set name");
        let package_fields = package.fields().collect::<Vec<_>>();

        assert!(package_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection }
                if name == "_Entitites" && entity == "Entity" && *collection
        )));

        let entity = ddic
            .active_entity_by_setname("EntitySet")
            .expect("active entity by set name");
        let entity_fields = entity.fields().collect::<Vec<_>>();

        assert!(entity_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection }
                if name == "_Fields" && entity == "Field" && *collection
        )));
        assert!(entity_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection }
                if name == "_Many" && entity == "Relation" && *collection
        )));
        assert!(entity_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection }
                if name == "package" && entity == "Package" && !*collection
        )));

        let field = ddic
            .active_entity_by_setname("FieldSet")
            .expect("active field by set name");
        let field_fields = field.fields().collect::<Vec<_>>();

        assert!(field_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection }
                if name == "entity" && entity == "Entity" && !*collection
        )));

        let package_by_name = ddic
            .active_entity("Package")
            .expect("active entity by name");
        assert_eq!(package_by_name.setname(), "PackageSet");
    }

    #[test]
    fn derives_navigation_info_from_active_entities() {
        let mut ddic = DataDictonary::new();
        ddic.activate();

        let entity = ddic.active_entity("Entity").expect("active entity");
        let fields = ddic
            .navigation_for_entity(entity, "_Fields")
            .expect("fields navigation");

        assert_eq!(fields.source_set, "EntitySet");
        assert_eq!(fields.target_entity, "Field");
        assert_eq!(fields.target_set, "FieldSet");
        assert!(fields.collection);
        assert_eq!(fields.foreign_key_field.as_deref(), Some("entity"));

        let field = ddic.active_entity("Field").expect("active field");
        let parent = ddic
            .navigation_for_entity(field, "entity")
            .expect("parent navigation");

        assert_eq!(parent.target_set, "EntitySet");
        assert!(!parent.collection);
        assert_eq!(parent.foreign_key_field.as_deref(), Some("entity"));
    }
}
