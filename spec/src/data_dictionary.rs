use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::system_user;
use crate::value_list::{self, ValueList, ITEMS_ENTITY};
use crate::{EntitySpec, FieldSpec, Relationship, app::App, package::Package};
use tracing::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataDictionary {
    packages: Vec<Package>,
    entities: Vec<EntitySpec>,
    relationships: Vec<Relationship>,
    apps: Vec<App>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    value_lists: Vec<ValueList>,
    #[serde(skip)]
    active: HashMap<String, EntitySpec>,
}

impl DataDictionary {
    pub fn new() -> Self {
        debug!("using default ddic");
        let _core = Package::new("core");
        let core = Package::new("core");

        let entity_ent = EntitySpec::new("Entity", core.name(), "Entities")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
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
            .field(FieldSpec::atom("immutable", "Immutable", "Edm.Boolean"))
            .field(FieldSpec::atom("multiline", "Multiline", "Edm.Boolean"));

        let relation_ent = EntitySpec::new("Relation", core.name(), "Relations")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::bool_field("owned", "Owned"))
            .field(FieldSpec::atom(
                "source_field",
                "List Reference",
                "Edm.String",
            ))
            .field(FieldSpec::atom(
                "target_field",
                "Single Field",
                "Edm.String",
            ));

        // `name` ist Titel und Listenspalte (Annotationen verweisen darauf); es traegt den Code.
        let value_ent = EntitySpec::new("Value", core.name(), "Values")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("value", "Value", "Edm.String"))
            .field(FieldSpec::atom("label", "Label", "Edm.String"))
            .field(FieldSpec::atom("sort", "Sort", "Edm.Int64"));
        let value_list_ent = EntitySpec::new("ValueList", core.name(), "Value Lists")
            .field(FieldSpec::atom("name", "Name", "Edm.String"));
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
            .with_one("Package", "_Entities");
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
        let r8 = Relationship::new("value_list_values", core.name())
            .with_one("ValueList", "_Values")
            .with_many("Value", "list");
        let r9 = Relationship::new("value_list_fields", core.name())
            .with_one("ValueList", "_Fields")
            .with_many("Field", "value_list");
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
        let a6 = App::new(
            "valuelists".into(),
            "Value Lists".into(),
            "ValueList".into(),
            Some("Maintain Value Lists".into()),
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
                value_list_ent,
            ],
            relationships: vec![rel, r2, r3, r4, r5, r6, r7, r8, r9],
            apps: vec![a1, a2, a3, a4, a5, a6],
            value_lists: Vec::new(),
            active: HashMap::new(),
        }
    }

    /// Ein leeres Dictionary - Basis fuer gefilterte Kopien (z.B. beim Export,
    /// wo das Metamodell selbst nicht mit in den Laufzeitserver soll).
    ///
    /// `new()` liefert dagegen immer das eincompilierte Metamodell.
    pub fn empty() -> Self {
        Self {
            packages: Vec::new(),
            entities: Vec::new(),
            relationships: Vec::new(),
            apps: Vec::new(),
            value_lists: Vec::new(),
            active: HashMap::new(),
        }
    }

    pub fn apps(&self) -> impl Iterator<Item = &App> {
        self.apps.iter()
    }

    pub fn packages(&self) -> impl Iterator<Item = &Package> {
        self.packages.iter()
    }

    pub fn value_lists(&self) -> &[ValueList] {
        &self.value_lists
    }

    /// Festwertlisten pruefen (siehe [`value_list::validate`]).
    pub fn validate_value_lists(&self) -> Result<(), String> {
        value_list::validate(&self.value_lists)
    }

    /// Generierte Entitaet mit allen Festwerten. Ohne Listen entsteht keine.
    fn generated_entities(&self) -> Vec<EntitySpec> {
        if self.value_lists.is_empty() {
            return Vec::new();
        }
        let mut items = EntitySpec::new(ITEMS_ENTITY, "core", "FieldValueListItems")
            // `name` ist Titel und Listenspalte; es traegt die Bezeichnung.
            .field(FieldSpec::atom("name", "Bezeichnung", "Edm.String"))
            .field(FieldSpec::atom("ListID", "Liste", "Edm.String"))
            .field(FieldSpec::atom("value", "Wert", "Edm.String"))
            .field(FieldSpec::atom("label", "Bezeichnung", "Edm.String"))
            .field(FieldSpec::atom("sort", "Sortierung", "Edm.Int64"));
        for field in system_user::audit_fields() {
            items = items.field(field);
        }
        vec![items]
    }

    /// Ergaenzt das Dictionary um die Benutzerverwaltung: Entitaet `SystemUser`,
    /// Audit-Felder (`Owner`, `CreatedBy`, `ChangedBy`, `CreatedAt`, `ChangedAt`)
    /// auf jeder Entitaet und die App fuer `SystemUser`.
    ///
    /// Idempotent. Eine in der Datei deklarierte `SystemUser` wird ergaenzt;
    /// gleichnamige Felder werden durch die Vorgabe ersetzt. Nur im Speicher.
    pub fn inject_system_user(&mut self) {
        use crate::system_user as su;

        self.add_package(Package::new(su::PACKAGE));

        let user_fields = [
            FieldSpec::string(su::USERNAME, "Name", 255),
            FieldSpec::string(su::NAME, "Full Name", 255),
            FieldSpec::bool_field(su::ACTIVE, "Active"),
        ];
        match self.entities.iter_mut().find(|e| e.name == su::SYSTEM_USER) {
            Some(entity) => {
                for field in user_fields {
                    replace_field(&mut entity.fields, field);
                }
            }
            None => {
                let mut entity =
                    EntitySpec::new(su::SYSTEM_USER, su::PACKAGE, "System Users");
                for field in user_fields {
                    entity = entity.field(field);
                }
                self.entities.push(entity);
            }
        }

        for entity in self.entities.iter_mut() {
            for field in su::audit_fields() {
                replace_field(&mut entity.fields, field);
            }
        }

        if !self.apps.iter().any(|a| a.name() == "systemusers") {
            self.apps.push(App::new(
                "systemusers".into(),
                "System Users".into(),
                su::SYSTEM_USER.into(),
                Some("Maintain System Users".into()),
                Some("sap-icon://person-placeholder".into()),
                Package::new(su::PACKAGE),
            ));
        }
    }

    pub fn entities(&self) -> Vec<EntitySpec> {
        let mut entities = self.entities.clone();
        entities.extend(self.generated_entities());
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
                trace!("adding one {}", relation.name);
                e.fields.push(FieldSpec::Lookup {
                    name: one_field.clone().unwrap(),
                    entity: many.clone().unwrap(),
                    collection: true,
                    owned: false,
                    label: relation.one_label.clone(),
                });
            }

            if let Some(e) = entities
                .iter_mut()
                .find(|x| &x.name == many.as_ref().unwrap())
            {
                trace!("adding many {}", relation.name);
                e.fields.push(FieldSpec::Lookup {
                    name: many_property.clone().unwrap(),
                    entity: one.clone().unwrap(),
                    collection: false,
                    owned: false,
                    label: relation.many_label.clone(),
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

    /// Registriert eine Entitaet, es sei denn, es existiert bereits eine mit dem gleichen Namen.
    pub fn add_entity(&mut self, entity: EntitySpec) -> bool {
        if self.entities.iter().any(|e| e.name() == entity.name()) {
            return false;
        }
        self.entities.push(entity);
        true
    }

    /// Registrier eine Beziehunge, falls keine mit gleichem Namen existiert.
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
                debug!("add field: {}", field.name());
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
            label: _,
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
                        ..
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
        for entitiy in self.entities.iter().chain(self.generated_entities().iter()) {
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
                        if !active.fields.iter().any(|x| x.name() == one_field) {
                            debug!("add one field {}", one_field);
                            active.fields.push(FieldSpec::Lookup {
                                name: one_field.clone(),
                                entity: many.clone(),
                                collection: true,
                                owned: false,
                                label: r.one_label.clone(),
                            });
                        }
                    }
                    if many == &active.name {
                        if !active.fields.iter().any(|x| x.name() == many_property) {
                            debug!("add many field {}", many_property);
                            active.fields.push(FieldSpec::Lookup {
                                name: many_property.clone(),
                                entity: one.clone(),
                                collection: false,
                                owned: false,
                                label: r.many_label.clone(),
                            });
                        }
                    }
                }
            }
            self.active.insert(active.name.clone(), active);
        }
    }
    /// Prueft die Listenansichten aller Entitaeten gegen den aktivierten Stand.
    ///
    /// Regeln: Ansichtsnamen eindeutig pro Entitaet, ohne `#` und Leerzeichen;
    /// mindestens eine Spalte; keine doppelten Spalten; jede Spalte ist ein
    /// Atom oder eine einwertige Navigation, deren Ziel ein Feld `name` hat.
    /// Sammelnavigationen sind als Spalte nicht zulaessig.
    pub fn validate_list_views(&self) -> Result<(), String> {
        let mut active = self.clone();
        active.activate();

        for entity in &self.entities {
            let mut seen_names: Vec<&str> = Vec::new();
            for view in entity.list_views() {
                let ctx = format!("{}.list_views['{}']", entity.name(), view.name);
                if view.name.trim().is_empty()
                    || view.name.contains('#')
                    || view.name.chars().any(char::is_whitespace)
                {
                    return Err(format!("{ctx}: ungueltiger Ansichtsname"));
                }
                if seen_names.contains(&view.name.as_str()) {
                    return Err(format!("{ctx}: Ansichtsname doppelt"));
                }
                seen_names.push(&view.name);
                if view.columns.is_empty() {
                    return Err(format!("{ctx}: keine Spalten"));
                }

                let mut seen_cols: Vec<&str> = Vec::new();
                for col in &view.columns {
                    if seen_cols.contains(&col.as_str()) {
                        return Err(format!("{ctx}: Spalte '{col}' doppelt"));
                    }
                    seen_cols.push(col);
                    check_list_column(&active, entity.name(), col)
                        .map_err(|e| format!("{ctx}: {e}"))?;
                }
            }
        }
        Ok(())
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

impl Default for DataDictionary {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
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
}

/// Ersetzt ein gleichnamiges Feld oder haengt es an.
fn replace_field(fields: &mut Vec<FieldSpec>, field: FieldSpec) {
    fields.retain(|f| f.name() != field.name());
    fields.push(field);
}

/// Prueft eine Listenspalte `col` der Entitaet `entity` (aktivierter Stand).
fn check_list_column(active: &DataDictionary, entity: &str, col: &str) -> Result<(), String> {
    let e = active
        .active_entity(entity)
        .ok_or_else(|| format!("Entitaet '{entity}' nicht aktiv"))?;
    let field = e
        .fields()
        .find(|f| f.name() == col)
        .ok_or_else(|| format!("Feld '{col}' existiert nicht"))?;
    match field {
        FieldSpec::Atom { .. } => Ok(()),
        FieldSpec::Lookup {
            entity: target,
            collection: false,
            ..
        } => {
            let has_name = active
                .active_entity(target)
                .is_some_and(|t| t.fields().any(|f| f.name() == "name"));
            if has_name {
                Ok(())
            } else {
                Err(format!("Ziel '{target}' von '{col}' hat kein Feld 'name'"))
            }
        }
        FieldSpec::Lookup { .. } => Err(format!(
            "'{col}' ist eine Sammelnavigation und kann keine Listenspalte sein"
        )),
        FieldSpec::Measure { .. } => Err(format!("Measure '{col}' wird nicht als Spalte unterstuetzt")),
    }
}

#[cfg(test)]
mod list_view_tests {
    use super::*;
    use crate::package::Package;
    use crate::ListView;

    fn atom(name: &str) -> FieldSpec {
        FieldSpec::Atom {
            name: name.into(),
            label: name.into(),
            edm_type: "Edm.String".into(),
            package: None,
            max_length: None,
            precision: None,
            scale: None,
            computed: false,
            immutable: false,
            value_list: None,
            link: false,
            multiline: false,
        }
    }

    /// Agent 1:n Video (Video.agent einwertig), Video 1:n Tag (Video._Tags Sammelnavigation).
    fn ddic(views: Vec<ListView>) -> DataDictionary {
        let mut ddic = DataDictionary::empty();
        ddic.add_package(Package::new("core"));
        ddic.add_entity(EntitySpec::new("Agent", "core", "Agents").field(atom("name")));
        let mut video = EntitySpec::new("Video", "core", "Videos")
            .field(atom("name"))
            .field(atom("file_size"));
        for v in views {
            video = video.list_view(v);
        }
        ddic.add_entity(video);
        ddic.add_entity(EntitySpec::new("Tag", "core", "Tags").field(atom("name")));
        ddic.add_relationship(
            Relationship::new("videos_agent_id", "core")
                .with_one("Agent", "_Videos")
                .with_many("Video", "agent"),
        );
        ddic.add_relationship(
            Relationship::new("tags_video_id", "core")
                .with_one("Video", "_Tags")
                .with_many("Tag", "video"),
        );
        ddic
    }

    #[test]
    fn valid_views_pass() {
        let d = ddic(vec![
            ListView::new("default", &["name", "agent", "file_size"]),
            ListView::new("review", &["file_size"]),
        ]);
        assert!(d.validate_list_views().is_ok());
    }

    #[test]
    fn unknown_column_is_rejected() {
        let d = ddic(vec![ListView::new("default", &["nope"])]);
        let err = d.validate_list_views().unwrap_err();
        assert!(err.contains("'nope' existiert nicht"), "{err}");
    }

    #[test]
    fn to_many_navigation_is_rejected_as_column() {
        let d = ddic(vec![ListView::new("default", &["_Tags"])]);
        let err = d.validate_list_views().unwrap_err();
        assert!(err.contains("Sammelnavigation"), "{err}");
    }

    #[test]
    fn duplicate_view_name_and_column_are_rejected() {
        let d = ddic(vec![
            ListView::new("default", &["name"]),
            ListView::new("default", &["name"]),
        ]);
        assert!(d.validate_list_views().unwrap_err().contains("doppelt"));

        let d = ddic(vec![ListView::new("x", &["name", "name"])]);
        assert!(d.validate_list_views().unwrap_err().contains("doppelt"));
    }

    #[test]
    fn invalid_name_and_empty_columns_are_rejected() {
        let d = ddic(vec![ListView::new("a#b", &["name"])]);
        assert!(d.validate_list_views().unwrap_err().contains("Ansichtsname"));

        let d = ddic(vec![ListView::new("x", &[])]);
        assert!(d.validate_list_views().unwrap_err().contains("keine Spalten"));
    }
}

