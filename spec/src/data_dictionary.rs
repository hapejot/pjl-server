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
            .field(FieldSpec::atom("name", "Name", "edm.String"))
            .field(FieldSpec::atom("package", "Package", "edm.String"))
            .field(FieldSpec::atom("plural", "Plural", "edm.String"));

        let _field_ent = EntitySpec::new("Field", core.name(), "Fields")
            .field(FieldSpec::atom("name", "Name", "edm.String"))
            .field(FieldSpec::atom("role", "Role", "edm.String"))
            .field(FieldSpec::atom("label", "Label", "edm.String"))
            .field(FieldSpec::atom("type", "Type", "edm.String"))
            .field(FieldSpec::atom("package", "Package", "edm.String"))
            .field(FieldSpec::atom("max_length", "MaxLength", "edm.Int"))
            .field(FieldSpec::atom("precision", "Precision", "edm.Int"))
            .field(FieldSpec::atom("scale", "Scale", "edm.Int"))
            .field(FieldSpec::atom("computed", "Package", "edm.Bool"))
            .field(FieldSpec::atom("immutable", "Immutable", "edm.Bool"))
            .field(FieldSpec::atom("package", "Package", "edm.String"));

        let field_ent = EntitySpec::new("Relation", core.name(), "Relations")
            .field(FieldSpec::atom("name", "Name", "edm.String"))
            .field(FieldSpec::atom("source", "Source", "edm.String"))
            .field(FieldSpec::atom("target", "Target", "edm.String"))
            .field(FieldSpec::atom("target_field", "TargetField", "edm.String"));

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

        Self {
            packages: vec![core],
            entities: vec![entity_ent, field_ent],
            relationships: vec![rel],
            apps: vec![a1, a2, a3],
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
