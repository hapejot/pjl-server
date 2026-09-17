//! Die Stellen, an denen sich SQL-Dialekte unterscheiden.
//!
//! Das Trait ist bewusst klein gehalten. Was in SQLite und anderen
//! Datenbanken gleich geschrieben wird - `lower`, `upper`, `trim`, `length`,
//! `round`, `substr`, die Verkettung mit `||` - steht unveraendert im
//! Uebersetzer. Aufgenommen ist nur, was tatsaechlich auseinanderlaeuft;
//! jede Methode hier ist ein belegter Unterschied, keine Vorsichtsmassnahme.

use data_store::schema::DataType;
use odata::query::SortDirection;

pub trait Dialect: std::fmt::Debug + Send + Sync {
    /// Platzhalter fuer den `index`-ten Parameter, 1-basiert.
    ///
    /// SQLite nummeriert mit `?1`, Postgres mit `$1`.
    fn placeholder(&self, index: usize) -> String;

    /// Speicherklasse bzw. Spaltentyp fuer einen Schematyp.
    fn column_type(&self, data_type: &DataType) -> &'static str;

    /// Wahrheitswert als Literal in einer DDL oder Projektion.
    ///
    /// SQLite kennt keinen Wahrheitstyp und schreibt 0/1.
    fn bool_literal(&self, value: bool) -> &'static str;

    /// Konstantes Praedikat, das immer bzw. nie zutrifft.
    fn bool_constant(&self, value: bool) -> String;

    /// Eine Bool-Spalte als Praedikat verwenden.
    ///
    /// In SQLite ist das ein Vergleich gegen 0/1, in Datenbanken mit echtem
    /// `BOOLEAN` steht die Spalte fuer sich.
    fn bool_predicate(&self, column: &str, expected: bool) -> String;

    /// `LIMIT`/`OFFSET`. SQLite verlangt ein LIMIT, sobald ein OFFSET
    /// gesetzt ist, und behilft sich mit `LIMIT -1`.
    fn paging(&self, top: Option<u32>, skip: Option<u32>) -> String;

    /// Sortierrichtung samt NULL-Behandlung.
    ///
    /// SQLite sortiert NULL aufsteigend zuerst - dieselbe Ordnung wie
    /// `data_store::query::compare_json`. Datenbanken, die NULL aufsteigend
    /// zuletzt einsortieren, muessen das hier ausgleichen.
    fn order_direction(&self, direction: SortDirection) -> String;

    /// Position von `needle` in `haystack`, 1-basiert, 0 wenn nicht enthalten.
    ///
    /// SQLite: `instr`, Postgres: `strpos`. Keine der beiden kennt die
    /// jeweils andere.
    fn position(&self, haystack: &str, needle: &str) -> String;

    /// Praedikat "endet auf".
    ///
    /// SQLite zaehlt bei `substr` mit negativem Offset vom Ende her;
    /// anderswo braucht es dafuer eine eigene Funktion.
    fn ends_with(&self, haystack: &str, needle: &str) -> String;
}

/// Dialekt von SQLite.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sqlite;

/// Der im Store verwendete Dialekt.
pub static SQLITE: Sqlite = Sqlite;

impl Dialect for Sqlite {
    fn placeholder(&self, index: usize) -> String {
        format!("?{index}")
    }

    /// `Decimal` wird bewusst als TEXT gehalten: SQLite kennt keinen exakten
    /// Dezimaltyp, und stilles Runden auf Betragsfeldern waere schlimmer als
    /// Arithmetik in Rust.
    fn column_type(&self, data_type: &DataType) -> &'static str {
        match data_type {
            DataType::Bool | DataType::Int32 | DataType::Int64 => "INTEGER",
            DataType::Binary => "BLOB",
            DataType::Guid
            | DataType::String { .. }
            | DataType::Date
            | DataType::DateTimeOffset
            | DataType::Decimal { .. } => "TEXT",
        }
    }

    fn bool_literal(&self, value: bool) -> &'static str {
        if value {
            "1"
        } else {
            "0"
        }
    }

    fn bool_constant(&self, value: bool) -> String {
        if value { "1 = 1" } else { "1 = 0" }.to_string()
    }

    fn bool_predicate(&self, column: &str, expected: bool) -> String {
        format!("{column} = {}", self.bool_literal(expected))
    }

    fn paging(&self, top: Option<u32>, skip: Option<u32>) -> String {
        match (top, skip) {
            (None, None) => String::new(),
            (Some(top), None) => format!(" LIMIT {top}"),
            (None, Some(skip)) => format!(" LIMIT -1 OFFSET {skip}"),
            (Some(top), Some(skip)) => format!(" LIMIT {top} OFFSET {skip}"),
        }
    }

    fn order_direction(&self, direction: SortDirection) -> String {
        match direction {
            SortDirection::Asc => "ASC".into(),
            SortDirection::Desc => "DESC".into(),
        }
    }

    fn position(&self, haystack: &str, needle: &str) -> String {
        format!("instr({haystack}, {needle})")
    }

    fn ends_with(&self, haystack: &str, needle: &str) -> String {
        format!("substr({haystack}, -length({needle})) = {needle}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_needs_a_limit_before_an_offset() {
        assert_eq!(SQLITE.paging(None, Some(5)), " LIMIT -1 OFFSET 5");
        assert_eq!(SQLITE.paging(Some(2), Some(5)), " LIMIT 2 OFFSET 5");
        assert_eq!(SQLITE.paging(None, None), "");
    }

    #[test]
    fn booleans_are_numbers_in_sqlite() {
        assert_eq!(SQLITE.bool_literal(true), "1");
        assert_eq!(SQLITE.bool_predicate("\"Flag\"", false), "\"Flag\" = 0");
    }
}
