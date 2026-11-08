use super::*;

#[test]
fn atom_accepts_known_format() {
    let json =
        r#"{"Atom":{"name":"size","label":"Size","edm_type":"Edm.Int64","ui":{"format":"bytes"}}}"#;
    let f: FieldSpec = serde_json::from_str(json).expect("gueltiger ui-Block");
    match f {
        FieldSpec::Atom { ui, .. } => assert_eq!(ui.unwrap().format, Some(DisplayFormat::Bytes)),
        _ => panic!("Atom erwartet"),
    }
}

#[test]
fn unknown_key_in_field_ui_is_an_error() {
    let json =
        r#"{"Atom":{"name":"size","label":"Size","edm_type":"Edm.Int64","ui":{"formt":"bytes"}}}"#;
    assert!(serde_json::from_str::<FieldSpec>(json).is_err());
}

#[test]
fn unknown_format_is_an_error() {
    let json = r#"{"Atom":{"name":"size","label":"Size","edm_type":"Edm.Int64","ui":{"format":"furlongs"}}}"#;
    assert!(serde_json::from_str::<FieldSpec>(json).is_err());
}

#[test]
fn unknown_key_in_entity_ui_is_an_error() {
    let ui: Result<EntityUi, _> = serde_json::from_str(r#"{"hidden_field":["x"]}"#);
    assert!(ui.is_err());
    let ok: EntityUi = serde_json::from_str(r#"{"hidden_fields":["x"]}"#).unwrap();
    assert_eq!(ok.hidden_fields, vec!["x".to_string()]);
}
