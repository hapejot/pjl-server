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

        // let mut package_fld = FieldSpec::atom("package", "Package", "Edm.Guid");
        // package_fld.lookup("Package");

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
            .field(FieldSpec::atom("max_length", "MaxLength", "Edm.Int64"))
            .field(FieldSpec::atom("precision", "Precision", "Edm.Int64"))
            .field(FieldSpec::atom("scale", "Scale", "Edm.Int64"))
            .field(FieldSpec::atom("computed", "Computed", "Edm.Boolean"))
            .field(FieldSpec::atom("immutable", "Immutable", "Edm.Boolean"));

        let relation_ent = EntitySpec::new("Relation", core.name(), "Relations")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::bool_field("owned", "Owned"))
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

        let app_ent = EntitySpec::new("App", core.name(), "Apps")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("title", "Title", "Edm.String"))
            .field(FieldSpec::atom("subtitle", "Subtitle", "Edm.String"))
            .field(FieldSpec::atom("icon", "Icon", "Edm.String"));

        let rel = Relationship::new("entity_fields", core.name())
            .with_one("Entity", "_Fields")
            .with_many("Field", "entity")
            .owned();
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
        let r6 = Relationship::new("app_entity", core.name())
            .with_one("Entity", "_Apps")
            .with_many("App", "main_entity");
        let r7 = Relationship::new("app_package", core.name())
            .with_one("Package", "_Apps")
            .with_many("App", "package");
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
        let a5 = App::new(
            "apps".into(),
            "Apps".into(),
            "App".into(),
            Some("Maintain Apps".into()),
            Some("sap-icon://world".into()),
            core.clone(),
        );

        Self {
            packages: vec![core],
            entities: vec![
                entity_ent,
                relation_ent,
                field_ent,
                value_ent,
                package_ent,
                app_ent,
            ],
            relationships: vec![rel, r2, r3, r4, r5, r6, r7],
            apps: vec![a1, a2, a3, a4, a5],
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
                    owned: false,
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
                    owned: false,
                });
            }
        }
        entities
    }

    pub fn relationships(&self) -> impl Iterator<Item = &Relationship> {
        self.relationships.iter()
    }

    /// Composition in der `entity` die abhaengige (N-)Seite ist.
    pub fn owning_relationship(&self, entity: &str) -> Option<&Relationship> {
        self.relationships
            .iter()
            .find(|r| r.is_owned() && r.many.as_deref() == Some(entity))
    }

    /// Compositions, deren Kinder an `entity` haengen.
    pub fn owned_children(&self, entity: &str) -> impl Iterator<Item = &Relationship> {
        self.relationships
            .iter()
            .filter(move |r| r.is_owned() && r.one.as_deref() == Some(entity))
    }

    /// Register a package unless one with the same name already exists.
    pub fn add_package(&mut self, package: Package) -> bool {
        if self.packages.iter().any(|p| p.name() == package.name()) {
            return false;
        }
        self.packages.push(package);
        true
    }

    /// Register an entity unless one with the same name already exists.
    pub fn add_entity(&mut self, entity: EntitySpec) -> bool {
        if self.entities.iter().any(|e| e.name() == entity.name()) {
            return false;
        }
        self.entities.push(entity);
        true
    }

    /// Register a relationship unless one with the same name already exists.
    pub fn add_relationship(&mut self, relationship: Relationship) -> bool {
        if self
            .relationships
            .iter()
            .any(|r| r.name() == relationship.name())
        {
            return false;
        }
        self.relationships.push(relationship);
        true
    }

    /// Register an app unless one with the same name already exists.
    pub fn add_app(&mut self, app: App) -> bool {
        if self.apps.iter().any(|a| a.name() == app.name()) {
            return false;
        }
        self.apps.push(app);
        true
    }

    /// Append a field to an already registered entity.
    ///
    /// Lookups are never added here - they are derived from the relationships.
    pub fn add_field(&mut self, entity: &str, field: FieldSpec) -> bool {
        if matches!(field, FieldSpec::Lookup { .. }) {
            return false;
        }
        match self.entities.iter_mut().find(|e| e.name() == entity) {
            Some(e) => {
                e.fields.push(field);
                true
            }
            None => false,
        }
    }

    /// Names of all registered entities, including the ones loaded at runtime.
    pub fn entity_names(&self) -> impl Iterator<Item = &str> {
        self.entities.iter().map(|e| e.name())
    }

    /// Resolve the foreign-key field on the target side of a `FieldSpec::Lookup`.
    ///
    /// For a scalar lookup (`collection == false`) this is just the lookup's own
    /// name. For a collection lookup we look up the matching `Relationship` (or
    /// fall back to a reverse scalar lookup on the target entity).
    pub fn foreign_key_field(&self, source: &EntitySpec, lookup: &FieldSpec) -> Option<String> {
        let FieldSpec::Lookup {
            name,
            entity: target,
            collection,
            owned: _,
        } = lookup
        else {
            return None;
        };

        if !collection {
            return Some(name.clone());
        }

        self.relationships()
            .find(|relationship| {
                relationship.one() == source.name()
                    && relationship.many() == target
                    && relationship.one_property() == name
            })
            .map(|relationship| relationship.many_property().to_string())
            .or_else(|| {
                let target_entity = self.resolve_entity(target)?;
                target_entity.fields().find_map(|field| match field {
                    FieldSpec::Lookup {
                        name: rev_name,
                        entity: rev_entity,
                        collection: false,
                        owned,
                    } if rev_entity == source.name() => Some(rev_name.clone()),
                    _ => None,
                })
            })
    }

    fn resolve_entity(&self, name: &str) -> Option<EntitySpec> {
        if self.active.is_empty() {
            self.entities().into_iter().find(|e| e.name() == name)
        } else {
            self.active.get(name).cloned()
        }
    }

    pub fn active_entity(&self, name: &str) -> Option<&EntitySpec> {
        self.active.get(name)
    }

    pub fn active_entities(&self) -> impl Iterator<Item = &EntitySpec> {
        self.active.values()
    }

    pub fn active_entity_by_setname(&self, set_name: &str) -> Option<&EntitySpec> {
        self.active
            .values()
            .find(|entity| entity.setname() == set_name)
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
                            owned: false,
                        });
                    }
                    if many == &active.name {
                        active.fields.push(FieldSpec::Lookup {
                            name: many_property.clone(),
                            entity: one.clone(),
                            collection: false,
                            owned: false,
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
            FieldSpec::Lookup { name, entity, collection, owned }
                if name == "_Entitites" && entity == "Entity" && *collection
        )));

        let entity = ddic
            .active_entity_by_setname("EntitySet")
            .expect("active entity by set name");
        let entity_fields = entity.fields().collect::<Vec<_>>();

        assert!(entity_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection, owned }
                if name == "_Fields" && entity == "Field" && *collection
        )));
        assert!(entity_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection, owned }
                if name == "_Many" && entity == "Relation" && *collection
        )));
        assert!(entity_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection, owned }
                if name == "package" && entity == "Package" && !*collection
        )));

        let field = ddic
            .active_entity_by_setname("FieldSet")
            .expect("active field by set name");
        let field_fields = field.fields().collect::<Vec<_>>();

        assert!(field_fields.iter().any(|field| matches!(
            field,
            FieldSpec::Lookup { name, entity, collection, owned }
                if name == "entity" && entity == "Entity" && !*collection
        )));

        let package_by_name = ddic
            .active_entity("Package")
            .expect("active entity by name");
        assert_eq!(package_by_name.setname(), "PackageSet");
    }

    #[test]
    fn derives_foreign_key_field_from_active_entities() {
        let mut ddic = DataDictonary::new();
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
}
