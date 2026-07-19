use crate::{EntitySpec, FieldSpec, Relationship, app::App, package::Package};

pub struct DataDictonary {
    packages: Vec<Package>,
    entities: Vec<EntitySpec>,
    relationships: Vec<Relationship>,
    apps: Vec<App>,
}

impl DataDictonary {
    pub fn new() -> Self {
        let _core = Package::new("core");
        let core = Package::new("core");

        let entity_ent = EntitySpec::new("Entity", core.name(), "Entities")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("package", "Package", "Edm.String"))
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
            .field(FieldSpec::atom("target", "Target", "Edm.String"))
            .field(FieldSpec::atom("target_field", "TargetField", "Edm.String"));

        let value_ent = EntitySpec::new("Value", core.name(), "Values")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("value", "Value", "Edm.String"))
            .field(FieldSpec::atom("label", "Label", "Edm.String"))
            .field(FieldSpec::atom("package", "Package", "Edm.String"));

        let package_ent = EntitySpec::new("Package", core.name(), "Packages")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("title", "Title", "Edm.String"))
            .field(FieldSpec::atom("description", "Description", "Edm.String"));

        let rel = Relationship::new("entity_fields", "Entity", "Field", "entity", core.name());

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
            relationships: vec![rel],
            apps: vec![a1, a2, a3, a4],
        }
    }

    pub fn apps(&self) -> impl Iterator<Item = &App> {
        self.apps.iter()
    }

    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }

    pub fn entities(&self) -> impl Iterator<Item = &EntitySpec> {
        self.entities.iter()
    }

    pub fn relationships(&self) -> impl Iterator<Item = &Relationship> {
        self.relationships.iter()
    }
}
