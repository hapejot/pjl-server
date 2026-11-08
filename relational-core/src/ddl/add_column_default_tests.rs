use super::*;

#[test]
fn added_column_carries_default_when_given() {
    let sql = add_column_sql("T", "Owner_ID", "TEXT", Some("'abc'"));
    assert_eq!(
        sql,
        "ALTER TABLE \"T\" ADD COLUMN \"Owner_ID\" TEXT DEFAULT 'abc'"
    );
}

#[test]
fn added_column_without_default_stays_plain() {
    let sql = add_column_sql("T", "Note", "TEXT", None);
    assert_eq!(sql, "ALTER TABLE \"T\" ADD COLUMN \"Note\" TEXT");
}
