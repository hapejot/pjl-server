use std::collections::HashMap;

use crate::{EntitySpec, FieldSpec, Relationship, app::App, package::Package};

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
            .with_one("Field", "entity")
            .with_many("Entity", "_Fields");
        let r2 = Relationship::new("entity_package", core.name())
            .with_many("Entity", "package")
            .with_one("Package", "_Entitites");
        let r3 = Relationship::new("many_entity", core.name())
            .with_one("Relation", "many")
            .with_many("Entity", "_Many");
        let r4 = Relationship::new("one_entity", core.name())
            .with_one("Relation", "one")
            .with_many("Entity", "_One");
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
                    entity: one.clone().unwrap(),
                    collection: true,
                });
            }

            if let Some(e) = entities
                .iter_mut()
                .find(|x| &x.name == many.as_ref().unwrap())
            {
                e.fields.push(FieldSpec::Lookup {
                    name: many_property.clone().unwrap(),
                    entity: many.clone().unwrap(),
                    collection: false,
                });
            }
        }
        entities
    }

    pub fn relationships(&self) -> impl Iterator<Item = &Relationship> {
        self.relationships.iter()
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
                            collection: true,
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
