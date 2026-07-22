use odata::path::{KeyLiteral, KeyValue};
use odata_yaml_store::EntityKey;

#[test]
fn convert_draft_key_from_json() {
    let e = EntityKey::new_from_json(&serde_json::json!({
        "ID": "00000000-0000-0000-0000-000000000001",
        "IsActiveEntity": false
    })).unwrap();

    assert_eq!(e.id.to_string(), "00000000-0000-0000-0000-000000000001");
    assert_eq!(e.is_active_entity, false);
}

#[test]
fn convert_draft_key_from_path_segment() {
    let e = EntityKey::from(&odata::path::PathSegment {
        name: "Entity".into(),
        keys: vec![KeyValue {
            name: Some("ID".into()),
            value: KeyLiteral::Guid("00000000-0000-0000-0000-000000000001".into()),
        },
        KeyValue {
            name: Some("IsActiveEntity".into()),
            value: KeyLiteral::Bool(false),
        }],
    });

    assert_eq!(e.id.to_string(), "00000000-0000-0000-0000-000000000001");
    assert_eq!(e.is_active_entity, false);
}