//! Adapter zwischen [`SqlParam`] und den Typen des Postgres-Treibers.
//!
//! Zwei Dinge sind hier anders als bei SQLite:
//!
//! 1. Postgres prueft Typen streng. Ein Wahrheitswert muss als `bool` an eine
//!    `BOOLEAN`-Spalte gehen; eine 0 oder 1 wird abgewiesen. Genau dafuer hat
//!    [`SqlParam`] einen eigenen Bool-Zweig.
//! 2. `ToSql` und `SqlParam` sind beide fremd, ein direkter `impl` also nicht
//!    moeglich. [`PgParam`] ist die noetige Huelle.

use std::error::Error;

use data_store::SqlParam;
use tokio_postgres::types::{to_sql_checked, FromSql, IsNull, ToSql, Type};
use tokio_postgres::Row;

/// Huelle um [`SqlParam`], damit der Treiber den Wert binden kann.
#[derive(Debug)]
pub struct PgParam<'a>(pub &'a SqlParam);

impl ToSql for PgParam<'_> {
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut tokio_postgres::types::private::BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match self.0 {
            SqlParam::Null => Ok(IsNull::Yes),
            SqlParam::Bool(b) => b.to_sql(ty, out),
            // Die Spalte entscheidet ueber die Breite: dieselbe Zahl kann an
            // INTEGER oder BIGINT gehen.
            SqlParam::Int(i) => match *ty {
                Type::INT2 => (*i as i16).to_sql(ty, out),
                Type::INT4 => (*i as i32).to_sql(ty, out),
                _ => i.to_sql(ty, out),
            },
            SqlParam::Real(f) => match *ty {
                Type::FLOAT4 => (*f as f32).to_sql(ty, out),
                _ => f.to_sql(ty, out),
            },
            SqlParam::Text(s) => s.as_str().to_sql(ty, out),
            SqlParam::Blob(b) => b.as_slice().to_sql(ty, out),
        }
    }

    /// Der Treiber fragt vorab, ob ein Typ bedient werden kann. Die Antwort
    /// haengt vom konkreten Wert ab, nicht vom Typ allein - deshalb wird hier
    /// pauschal zugestimmt und erst in [`ToSql::to_sql`] entschieden.
    fn accepts(_ty: &Type) -> bool {
        true
    }

    to_sql_checked!();
}

/// Liest eine Spalte in die backend-unabhaengige Darstellung.
///
/// Der Postgres-Typ der Spalte entscheidet, nicht der Schematyp: die
/// Rueckuebersetzung nach JSON macht [`data_store::param_to_json`] anhand des
/// Schemas.
pub fn from_postgres(row: &Row, index: usize) -> Result<SqlParam, data_store::StoreError> {
    let column_type = row.columns()[index].type_().clone();
    let read = |p: Option<SqlParam>| p.unwrap_or(SqlParam::Null);
    let err = |e: tokio_postgres::Error| {
        data_store::StoreError::Conversion(format!(
            "Spalte '{}' ({}): {e}",
            row.columns()[index].name(),
            column_type
        ))
    };

    Ok(match column_type {
        Type::BOOL => read(row.try_get::<_, Option<bool>>(index).map_err(err)?.map(SqlParam::Bool)),
        Type::INT2 => read(row
            .try_get::<_, Option<i16>>(index)
            .map_err(err)?
            .map(|v| SqlParam::Int(v.into()))),
        Type::INT4 => read(row
            .try_get::<_, Option<i32>>(index)
            .map_err(err)?
            .map(|v| SqlParam::Int(v.into()))),
        Type::INT8 => read(row.try_get::<_, Option<i64>>(index).map_err(err)?.map(SqlParam::Int)),
        Type::FLOAT4 => read(row
            .try_get::<_, Option<f32>>(index)
            .map_err(err)?
            .map(|v| SqlParam::Real(v.into()))),
        Type::FLOAT8 => read(row.try_get::<_, Option<f64>>(index).map_err(err)?.map(SqlParam::Real)),
        Type::BYTEA => read(row
            .try_get::<_, Option<Vec<u8>>>(index)
            .map_err(err)?
            .map(SqlParam::Blob)),
        _ => read(row
            .try_get::<_, Option<PgText>>(index)
            .map_err(err)?
            .map(|v| SqlParam::Text(v.0))),
    })
}

/// Nimmt jeden Typ als Text entgegen.
///
/// Die abgeleiteten Spalten einer Abfrage - etwa `HasDraftEntity` aus einem
/// `CASE WHEN` - tragen nicht immer den Typ, den das Schema erwarten laesst.
/// Statt jede Moeglichkeit aufzuzaehlen, wird der Rest als Zeichenkette
/// gelesen; [`data_store::param_to_json`] bringt ihn anhand des Schematyps
/// wieder in Form.
#[derive(Debug)]
struct PgText(String);

impl<'a> FromSql<'a> for PgText {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(PgText(match *ty {
            Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::UNKNOWN => {
                String::from_utf8_lossy(raw).to_string()
            }
            _ => String::from_utf8_lossy(raw).to_string(),
        }))
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use data_store::{json_to_param, DataType};
    use serde_json::json;

    /// Der Wahrheitswert kommt als `bool` an der Treibergrenze an - nicht als
    /// 0/1 wie bei SQLite.
    #[test]
    fn booleans_stay_booleans() {
        let param = json_to_param(&json!(true), &DataType::Bool, "IsActiveEntity").unwrap();
        assert_eq!(param, SqlParam::Bool(true));
        assert!(<PgParam as ToSql>::accepts(&Type::BOOL));
    }
}
