use super::*;
use serde_json::json;

#[test]
fn valid_uuid_id_is_kept() {
    let r = DataRecord::from_json(&json!({
        "ID": "00000000-0000-0000-0000-000000000042",
        "IsActiveEntity": true
    }))
    .unwrap();
    assert_eq!(r.id.to_string(), "00000000-0000-0000-0000-000000000042");
}

#[test]
fn text_id_is_kept_as_text() {
    let r = DataRecord::from_json(&json!({ "ID": "active", "IsActiveEntity": true })).unwrap();
    assert_eq!(r.id(), KeyValue::Text("active".into()));
}

#[test]
fn non_string_id_is_rejected_not_replaced() {
    let err = DataRecord::from_json(&json!({ "ID": 3963451, "IsActiveEntity": true }));
    assert!(matches!(err, Err(StoreError::Serialize(_))));
}

#[test]
fn missing_id_gets_new_uuid() {
    let r = DataRecord::from_json(&json!({ "IsActiveEntity": false })).unwrap();
    assert!(!r.is_active_entity);
}
