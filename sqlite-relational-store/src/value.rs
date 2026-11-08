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
mod tests;
