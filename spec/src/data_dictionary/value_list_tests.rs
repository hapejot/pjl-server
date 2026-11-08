use super::*;
use crate::value_list::{ValueEntry, ValueList, entry_id, is_valid_entity_name};
use crate::{AtomValueList, EntitySpec, FieldSpec};
use uuid::Uuid;

const LIST: &str = "79714635-4b3e-51a6-be78-dc009beb5f3c";

fn dictionary() -> DataDictionary {
    let mut dd = DataDictionary::empty();
    dd.value_lists = vec![ValueList {
        id: LIST.into(),
        name: "ProjectStatus".into(),
        values: vec![
            ValueEntry {
                code: "open".into(),
                label: "Offen".into(),
                sort: 1,
            },
            ValueEntry {
                code: "done".into(),
                label: "Erledigt".into(),
                sort: 2,
            },
        ],
        code_key: false,
    }];
    dd.entities
        .push(EntitySpec::new("Project", "core", "Projects").field(
            FieldSpec::atom("status", "Status", "Edm.String").with_value_list(
                AtomValueList::FieldValueList {
                    list_id: LIST.into(),
                    prefer_dialog: false,
                },
            ),
        ));
    dd
}

#[test]
fn entry_id_is_stable_and_depends_on_list_and_code() {
    assert_eq!(entry_id(LIST, "open"), entry_id(LIST, "open"));
    assert_ne!(entry_id(LIST, "open"), entry_id(LIST, "done"));
    assert_ne!(
        entry_id(LIST, "open"),
        entry_id(&Uuid::new_v4().to_string(), "open")
    );
}

#[test]
fn value_list_becomes_entity_per_list() {
    let dd = dictionary();
    let names: Vec<String> = dd.entities().iter().map(|e| e.name.clone()).collect();
    assert!(names.contains(&"ProjectStatus".to_string()));
    assert!(!names.contains(&"FieldValueListItems".to_string()));
}

#[test]
fn atom_with_value_list_becomes_lookup_in_memory_only() {
    let mut dd = dictionary();
    dd.apply_value_list_lookups().unwrap();
    let project = dd
        .entities()
        .into_iter()
        .find(|e| e.name == "Project")
        .unwrap();
    let field = project
        .fields()
        .find(|f| f.name() == "status")
        .unwrap()
        .clone();
    assert!(matches!(
        field,
        FieldSpec::Lookup { ref entity, collection: false, .. } if entity == "ProjectStatus"
    ));
    assert_eq!(dd.converted_value_fields().len(), 1);
    // Die Quelle (ddic.json) kennt weiterhin das Atom mit Festwertliste.
    let source = serde_json::to_string(&dictionary()).unwrap();
    assert!(source.contains("FieldValueList"));
}

#[test]
fn unknown_list_is_an_error() {
    let mut dd = dictionary();
    dd.value_lists.clear();
    assert!(dd.apply_value_list_lookups().is_err());
}

#[test]
fn list_name_must_be_an_entity_name() {
    assert!(is_valid_entity_name("ProjectStatus"));
    assert!(!is_valid_entity_name("Project Status"));
    assert!(!is_valid_entity_name("1Status"));
}
