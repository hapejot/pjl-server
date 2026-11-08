use super::*;
use serde_json::json;

#[test]
fn decimals_round_trip_exactly() {
    let ty = DataType::Decimal {
        precision: 15,
        scale: 3,
    };
    let param = json_to_param(&json!(123.456), &ty, "Amount").unwrap();
    assert_eq!(param, SqlParam::Text("123.456".into()));
    // Ausgabe als String: exakt, ohne Gleitkomma-Darstellung.
    assert_eq!(param_to_json(&param, &ty), json!("123.456"));
}

/// Der Wahrheitswert bleibt bis zur Treibergrenze erhalten. Ob er als 0/1
/// oder als BOOLEAN in der Datenbank landet, entscheidet das Backend.
#[test]
fn booleans_stay_booleans() {
    let param = json_to_param(&json!(true), &DataType::Bool, "IsActiveEntity").unwrap();
    assert_eq!(param, SqlParam::Bool(true));
    assert_eq!(param_to_json(&param, &DataType::Bool), json!(true));
}

/// SQLite liefert fuer eine Bool-Spalte eine Zahl zurueck - das Schema
/// entscheidet, dass daraus wieder ein Wahrheitswert wird.
#[test]
fn integers_read_back_as_booleans_when_the_column_says_so() {
    assert_eq!(
        param_to_json(&SqlParam::Int(0), &DataType::Bool),
        json!(false)
    );
    assert_eq!(
        param_to_json(&SqlParam::Int(1), &DataType::Bool),
        json!(true)
    );
    assert_eq!(param_to_json(&SqlParam::Int(1), &DataType::Int32), json!(1));
}

#[test]
fn type_mismatch_is_reported_with_the_column() {
    let err = json_to_param(&json!({"a": 1}), &DataType::Int32, "Amount").unwrap_err();
    assert!(err.to_string().contains("Amount"), "{err}");
}

#[test]
fn null_is_accepted_for_every_type() {
    for ty in [DataType::Guid, DataType::Int32, DataType::Bool] {
        assert_eq!(
            json_to_param(&Value::Null, &ty, "x").unwrap(),
            SqlParam::Null
        );
    }
}
