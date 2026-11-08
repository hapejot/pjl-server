use super::*;

const LIST: &str = "79714635-4b3e-51a6-be78-dc009beb5f3c";

fn dictionary(code_key: bool) -> DataDictionary {
    let mut dd = DataDictionary::empty();
    dd.value_lists = vec![ValueList {
        id: LIST.into(),
        name: "ProjectStatus".into(),
        values: vec![crate::value_list::ValueEntry {
            code: "open".into(),
            label: "Offen".into(),
            sort: 1,
        }],
        code_key,
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
    dd.apply_value_list_lookups().unwrap();
    dd.activate();
    dd
}

#[test]
fn code_key_list_is_keyed_by_code_without_drafts() {
    let dd = dictionary(true);
    let list = dd.active_entity("ProjectStatus").expect("list entity");
    assert_eq!(list.primary_key_name(), "value");
    assert_eq!(list.primary_key_type(), "Edm.String");
    assert_eq!(list.key_fields(), vec!["value"]);
    assert!(!list.draft_enabled);
    assert_eq!(dd.fk_column("ProjectStatus", "status"), "status_code");
    assert!(dd.validate_primary_keys().is_ok());
}

#[test]
fn guid_list_keeps_id_and_fk_suffix() {
    let dd = dictionary(false);
    let list = dd.active_entity("ProjectStatus").expect("list entity");
    assert_eq!(list.key_fields(), vec!["ID", "IsActiveEntity"]);
    assert_eq!(dd.fk_column("ProjectStatus", "status"), "status_ID");
}
