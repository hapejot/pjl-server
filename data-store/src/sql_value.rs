//! Backend-unabhaengiger Parametertyp fuer SQL-Abfragen.
//!
//! Die Uebersetzung von OData nach SQL kennt den Zieltreiber nicht. Sie
//! bindet Werte als [`SqlParam`]; jedes Backend rechnet sie erst an der
//! Treibergrenze in seine eigene Darstellung um.
//!
//! Wichtig ist der eigene [`SqlParam::Bool`]-Zweig. SQLite kennt keinen
//! Wahrheitstyp und legt Boolesche Werte als 0/1 ab, andere Datenbanken haben
//! einen echten `BOOLEAN` und weisen eine Zahl an dieser Stelle zurueck. Wer
//! die Unterscheidung schon beim Binden verliert, kann sie spaeter nicht mehr
//! herstellen.

use serde_json::{Number, Value};

use crate::schema::DataType;
use crate::StoreError;

/// Ein gebundener Wert, unabhaengig vom Datenbanktreiber.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlParam {
    Null,
    Bool(bool),
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl SqlParam {
    pub fn is_null(&self) -> bool {
        matches!(self, SqlParam::Null)
    }
}

/// JSON -> Parameter anhand des deklarierten Spaltentyps.
///
/// Anders als im Dokumentenstore ist der Typ hier bekannt - die Umwandlung
/// richtet sich nach dem Schema, nicht nach dem zufaelligen JSON-Typ.
pub fn json_to_param(
    value: &Value,
    data_type: &DataType,
    column: &str,
) -> Result<SqlParam, StoreError> {
    if value.is_null() {
        return Ok(SqlParam::Null);
    }
    let type_error = |expected: &str| {
        StoreError::Conversion(format!(
            "Spalte '{column}': {expected} erwartet, aber {value} erhalten"
        ))
    };

    Ok(match data_type {
        DataType::Bool => match value {
            Value::Bool(b) => SqlParam::Bool(*b),
            Value::Number(n) => SqlParam::Bool(n.as_i64().unwrap_or(0) != 0),
            _ => return Err(type_error("Boolean")),
        },
        DataType::Int32 | DataType::Int64 => match value {
            Value::Number(n) => match n.as_i64() {
                Some(i) => SqlParam::Int(i),
                None => SqlParam::Real(n.as_f64().unwrap_or_default()),
            },
            Value::Bool(b) => SqlParam::Int(i64::from(*b)),
            Value::String(s) => s
                .parse::<i64>()
                .map(SqlParam::Int)
                .map_err(|_| type_error("ganze Zahl"))?,
            _ => return Err(type_error("ganze Zahl")),
        },
        // Exakt als Text halten - keine Gleitkomma-Rundung auf Betraegen.
        DataType::Decimal { .. } => match value {
            Value::Number(n) => SqlParam::Text(n.to_string()),
            Value::String(s) => SqlParam::Text(s.clone()),
            _ => return Err(type_error("Dezimalzahl")),
        },
        // Leerer Zeitstempel ist kein Wert: sonst liest UI5 "" und meldet einen Fehler.
        DataType::DateTimeOffset if matches!(value, Value::String(s) if s.trim().is_empty()) => {
            SqlParam::Null
        }
        DataType::Guid | DataType::String { .. } | DataType::Date | DataType::DateTimeOffset => {
            match value {
                Value::String(s) => SqlParam::Text(s.clone()),
                Value::Number(n) => SqlParam::Text(n.to_string()),
                Value::Bool(b) => SqlParam::Text(b.to_string()),
                _ => return Err(type_error("Text")),
            }
        }
        DataType::Binary => match value {
            Value::String(s) => SqlParam::Text(s.clone()),
            _ => return Err(type_error("Binaerwert")),
        },
    })
}

/// JSON -> Parameter ohne Spaltenkontext (Filterliterale).
pub fn json_to_param_loose(value: &Value) -> SqlParam {
    match value {
        Value::Null => SqlParam::Null,
        Value::Bool(b) => SqlParam::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => SqlParam::Int(i),
            None => SqlParam::Real(n.as_f64().unwrap_or_default()),
        },
        Value::String(s) => SqlParam::Text(s.clone()),
        other => SqlParam::Text(other.to_string()),
    }
}

/// Parameter -> JSON anhand des deklarierten Spaltentyps.
///
/// Der Spaltentyp entscheidet, nicht die Speicherklasse: SQLite liefert fuer
/// eine Bool-Spalte eine Zahl, Postgres einen Wahrheitswert - herauskommen
/// muss in beiden Faellen [`Value::Bool`].
pub fn param_to_json(value: &SqlParam, data_type: &DataType) -> Value {
    match value {
        SqlParam::Null => Value::Null,
        SqlParam::Bool(b) => Value::Bool(*b),
        SqlParam::Int(i) => match data_type {
            DataType::Bool => Value::Bool(*i != 0),
            _ => Value::Number((*i).into()),
        },
        SqlParam::Real(f) => Number::from_f64(*f)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        SqlParam::Text(text) => match data_type {
            // Dezimalzahlen liegen als Text vor; als Zahl herausgeben,
            // damit Vergleiche und Sortierung wie im Dokumentenstore
            // funktionieren.
            DataType::Decimal { .. } => text
                .parse::<f64>()
                .ok()
                .and_then(Number::from_f64)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(text.clone())),
            DataType::Bool => Value::Bool(!text.is_empty() && text != "0" && text != "false"),
            // Leere Zeitstempel (z. B. aus frueheren Schreibvorgaengen) sind null.
            DataType::DateTimeOffset if text.trim().is_empty() => Value::Null,
            _ => Value::String(text.clone()),
        },
        SqlParam::Blob(bytes) => Value::String(String::from_utf8_lossy(bytes).to_string()),
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
        let param = json_to_param(&json!(123.456), &ty, "Amount").unwrap();
        assert_eq!(param, SqlParam::Text("123.456".into()));
        assert_eq!(param_to_json(&param, &ty), json!(123.456));
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
        assert_eq!(param_to_json(&SqlParam::Int(0), &DataType::Bool), json!(false));
        assert_eq!(param_to_json(&SqlParam::Int(1), &DataType::Bool), json!(true));
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
            assert_eq!(json_to_param(&Value::Null, &ty, "x").unwrap(), SqlParam::Null);
        }
    }
}
