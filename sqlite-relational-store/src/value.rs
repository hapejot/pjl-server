//! Konvertierung zwischen JSON-Werten und typisierten SQLite-Spalten.
//!
//! Anders als im Dokumentenstore ist der Typ hier bekannt - die Umwandlung
//! richtet sich nach dem Schema, nicht nach dem zufaelligen JSON-Typ.

use data_store::schema::DataType;
use data_store::StoreError;
use rusqlite::types::{Value as SqlValue, ValueRef};
use serde_json::{Number, Value};

/// JSON -> SQLite anhand des deklarierten Spaltentyps.
pub fn json_to_sql(value: &Value, data_type: &DataType, column: &str) -> Result<SqlValue, StoreError> {
    if value.is_null() {
        return Ok(SqlValue::Null);
    }
    let type_error = |expected: &str| {
        StoreError::Conversion(format!(
            "Spalte '{column}': {expected} erwartet, aber {value} erhalten"
        ))
    };

    Ok(match data_type {
        DataType::Bool => match value {
            Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
            Value::Number(n) => SqlValue::Integer(i64::from(n.as_i64().unwrap_or(0) != 0)),
            _ => return Err(type_error("Boolean")),
        },
        DataType::Int32 | DataType::Int64 => match value {
            Value::Number(n) => match n.as_i64() {
                Some(i) => SqlValue::Integer(i),
                None => SqlValue::Real(n.as_f64().unwrap_or_default()),
            },
            Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
            Value::String(s) => s
                .parse::<i64>()
                .map(SqlValue::Integer)
                .map_err(|_| type_error("ganze Zahl"))?,
            _ => return Err(type_error("ganze Zahl")),
        },
        // Exakt als Text halten - keine Gleitkomma-Rundung auf Betraegen.
        DataType::Decimal { .. } => match value {
            Value::Number(n) => SqlValue::Text(n.to_string()),
            Value::String(s) => SqlValue::Text(s.clone()),
            _ => return Err(type_error("Dezimalzahl")),
        },
        DataType::Guid | DataType::String { .. } | DataType::Date | DataType::DateTimeOffset => {
            match value {
                Value::String(s) => SqlValue::Text(s.clone()),
                Value::Number(n) => SqlValue::Text(n.to_string()),
                Value::Bool(b) => SqlValue::Text(b.to_string()),
                _ => return Err(type_error("Text")),
            }
        }
        DataType::Binary => match value {
            Value::String(s) => SqlValue::Text(s.clone()),
            _ => return Err(type_error("Binaerwert")),
        },
    })
}

/// JSON -> SQLite ohne Spaltenkontext (Filterliterale).
pub fn json_to_sql_loose(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(i64::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlValue::Integer(i),
            None => SqlValue::Real(n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

/// SQLite -> JSON anhand des deklarierten Spaltentyps.
pub fn sql_to_json(value: ValueRef<'_>, data_type: &DataType) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(i) => match data_type {
            DataType::Bool => Value::Bool(i != 0),
            _ => Value::Number(i.into()),
        },
        ValueRef::Real(f) => Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null),
        ValueRef::Text(bytes) => {
            let text = String::from_utf8_lossy(bytes).to_string();
            match data_type {
                // Dezimalzahlen liegen als Text vor; als Zahl herausgeben,
                // damit Vergleiche und Sortierung wie im Dokumentenstore
                // funktionieren.
                DataType::Decimal { .. } => text
                    .parse::<f64>()
                    .ok()
                    .and_then(Number::from_f64)
                    .map(Value::Number)
                    .unwrap_or(Value::String(text)),
                _ => Value::String(text),
            }
        }
        ValueRef::Blob(bytes) => Value::String(String::from_utf8_lossy(bytes).to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decimals_round_trip_exactly() {
        let ty = DataType::Decimal {
            precision: 15,
            scale: 3,
        };
        let sql = json_to_sql(&json!(123.456), &ty, "Amount").unwrap();
        assert_eq!(sql, SqlValue::Text("123.456".into()));
        assert_eq!(
            sql_to_json(ValueRef::Text(b"123.456"), &ty),
            json!(123.456)
        );
    }

    #[test]
    fn booleans_are_stored_as_integers() {
        let sql = json_to_sql(&json!(true), &DataType::Bool, "IsActiveEntity").unwrap();
        assert_eq!(sql, SqlValue::Integer(1));
        assert_eq!(sql_to_json(ValueRef::Integer(0), &DataType::Bool), json!(false));
    }

    #[test]
    fn type_mismatch_is_reported_with_the_column() {
        let err = json_to_sql(&json!({"a": 1}), &DataType::Int32, "Amount").unwrap_err();
        assert!(err.to_string().contains("Amount"), "{err}");
    }

    #[test]
    fn null_is_accepted_for_every_type() {
        for ty in [DataType::Guid, DataType::Int32, DataType::Bool] {
            assert_eq!(json_to_sql(&Value::Null, &ty, "x").unwrap(), SqlValue::Null);
        }
    }
}
