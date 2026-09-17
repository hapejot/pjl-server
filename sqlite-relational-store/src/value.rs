//! Adapter zwischen [`SqlParam`] und der Darstellung von rusqlite.
//!
//! Die typgerechte Umwandlung von und nach JSON liegt in
//! [`data_store::sql_value`] und ist fuer alle relationalen Backends dieselbe.
//! Hier bleibt nur, was wirklich treiberspezifisch ist.

use data_store::SqlParam;
use rusqlite::types::{Value as SqlValue, ValueRef};

/// Parameter an die Treibergrenze reichen.
///
/// SQLite kennt keinen Wahrheitstyp - `Bool` wird zu 0/1. Das ist die
/// Speicherklasse, die [`crate::ddl::sqlite_type`] fuer `DataType::Bool`
/// vergibt.
pub fn to_sqlite(param: &SqlParam) -> SqlValue {
    match param {
        SqlParam::Null => SqlValue::Null,
        SqlParam::Bool(b) => SqlValue::Integer(i64::from(*b)),
        SqlParam::Int(i) => SqlValue::Integer(*i),
        SqlParam::Real(f) => SqlValue::Real(*f),
        SqlParam::Text(s) => SqlValue::Text(s.clone()),
        SqlParam::Blob(b) => SqlValue::Blob(b.clone()),
    }
}

/// Gelesenen Wert in die backend-unabhaengige Darstellung heben.
///
/// Es entsteht nie ein [`SqlParam::Bool`]: SQLite liefert fuer eine
/// Bool-Spalte eine Zahl. Den Wahrheitswert stellt
/// [`data_store::param_to_json`] anhand des Spaltentyps wieder her.
pub fn from_sqlite(value: ValueRef<'_>) -> SqlParam {
    match value {
        ValueRef::Null => SqlParam::Null,
        ValueRef::Integer(i) => SqlParam::Int(i),
        ValueRef::Real(f) => SqlParam::Real(f),
        ValueRef::Text(bytes) => SqlParam::Text(String::from_utf8_lossy(bytes).to_string()),
        ValueRef::Blob(bytes) => SqlParam::Blob(bytes.to_vec()),
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(param_to_json(&from_sqlite(ValueRef::Text(b"123.456")), &ty), json!(123.456));
    }
}
