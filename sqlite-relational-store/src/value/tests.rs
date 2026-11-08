use super::*;
use data_store::{json_to_param, param_to_json, DataType};
use serde_json::json;

#[test]
fn booleans_are_stored_as_integers() {
    let param = json_to_param(&json!(true), &DataType::Bool, "IsActiveEntity").unwrap();
    assert_eq!(to_sqlite(&param), SqlValue::Integer(1));
}

/// Rundlauf ueber die Treibergrenze: ein Wahrheitswert kommt als
/// Wahrheitswert zurueck, obwohl SQLite dazwischen eine Zahl sieht.
#[test]
fn booleans_survive_the_round_trip() {
    for value in [json!(true), json!(false)] {
        let param = json_to_param(&value, &DataType::Bool, "IsActiveEntity").unwrap();
        let SqlValue::Integer(i) = to_sqlite(&param) else {
            panic!("SQLite legt Bool als Zahl ab");
        };
        let back = from_sqlite(ValueRef::Integer(i));
        assert_eq!(param_to_json(&back, &DataType::Bool), value);
    }
}

#[test]
fn decimals_stay_text_on_the_way_in() {
    let ty = DataType::Decimal {
        precision: 15,
        scale: 3,
    };
    let param = json_to_param(&json!(123.456), &ty, "Amount").unwrap();
    assert_eq!(to_sqlite(&param), SqlValue::Text("123.456".into()));
    // Dezimalzahlen gehen als JSON-String hinaus (exakt, siehe data-store/src/sql_value.rs).
    assert_eq!(
        param_to_json(&from_sqlite(ValueRef::Text(b"123.456")), &ty),
        json!("123.456")
    );
}
