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

/// Mit echtem BOOLEAN steht die Spalte fuer sich - ein Vergleich gegen
/// 0/1 waere ein Typfehler.
#[test]
fn booleans_are_a_type_of_their_own_in_postgres() {
    assert_eq!(POSTGRES.bool_literal(true), "TRUE");
    assert_eq!(POSTGRES.bool_predicate("\"Flag\"", true), "\"Flag\"");
    assert_eq!(POSTGRES.bool_predicate("\"Flag\"", false), "NOT \"Flag\"");
    assert_eq!(POSTGRES.bool_constant(false), "FALSE");
}

/// Postgres braucht kein Ersatz-LIMIT vor einem OFFSET.
#[test]
fn postgres_takes_an_offset_without_a_limit() {
    assert_eq!(POSTGRES.paging(None, Some(5)), " OFFSET 5");
    assert_eq!(POSTGRES.paging(Some(2), Some(5)), " LIMIT 2 OFFSET 5");
}

/// Die NULL-Ordnung muss zwischen den Backends uebereinstimmen, sonst
/// liefert dieselbe Abfrage je nach Datenbank eine andere Reihenfolge.
#[test]
fn both_dialects_sort_nulls_first_when_ascending() {
    assert_eq!(SQLITE.order_direction(SortDirection::Asc), "ASC");
    assert_eq!(
        POSTGRES.order_direction(SortDirection::Asc),
        "ASC NULLS FIRST"
    );
}

#[test]
fn placeholders_are_numbered_differently() {
    assert_eq!(SQLITE.placeholder(3), "?3");
    assert_eq!(POSTGRES.placeholder(3), "$3");
}
