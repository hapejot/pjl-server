use data_store::{Column, DataType, EntitySet};
use odata::query::Expr;

use crate::dialect::SQLITE;
use crate::sql::query::Translator;

fn order_set() -> EntitySet {
    EntitySet::new("OrderSet", "Order")
        .drafts()
        .column(Column::new("name", DataType::String { max_length: None }))
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn full_statement() -> anyhow::Result<()> {
    let set = order_set();
    let s = Translator::new(&set, &SQLITE)
        .select("ID")
        .select("IsActiveEntity")
        .select("HasActiveEntity")
        .select("HasDraftEntity")
        .select("name")
        .filter(
            &Expr::parse("(IsActiveEntity eq false or SiblingEntity/IsActiveEntity eq null)")
                .expect("parst"),
        )
        .build()?;

    let expected = normalize(
        r#"SELECT "ID", "IsActiveEntity", "HasActiveEntity", "HasDraftEntity", "name"
               FROM (SELECT *,
                   CASE WHEN "SiblingEntity/IsActiveEntity" = 0 OR "IsActiveEntity" = 0 THEN 1 ELSE 0 END AS "HasDraftEntity",
                   CASE WHEN "SiblingEntity/IsActiveEntity" = 1 THEN 1 ELSE 0 END AS "HasActiveEntity"
                 FROM (
                   SELECT a.*,
                     d."IsActiveEntity" AS "SiblingEntity/IsActiveEntity",
                     d."DraftAdministrativeData_ID" AS "SiblingEntity/DraftAdministrativeData_ID"
                   FROM "OrderSet" AS a
                   LEFT JOIN "OrderSet" AS d
                     ON a."ID" = d."ID" AND a."IsActiveEntity" <> d."IsActiveEntity" ))
               WHERE (("IsActiveEntity" = ?1) OR ("SiblingEntity/IsActiveEntity" IS NULL))"#,
    );
    assert_eq!(s.text, expected);
    Ok(())
}

/// Der Shadow-Filter ist wortgleich mit dem Draft-Filter, den Fiori
/// selbst schickt - beide Wege muessen dasselbe Praedikat erzeugen.
#[test]
fn shadow_matches_the_fiori_draft_filter() -> anyhow::Result<()> {
    let set = order_set();
    let implicit = Translator::new(&set, &SQLITE).shadow_drafts().build()?;
    let explicit = Translator::new(&set, &SQLITE)
        .filter(
            &Expr::parse("(IsActiveEntity eq false or SiblingEntity/IsActiveEntity eq null)")
                .expect("parst"),
        )
        .build()?;
    assert!(
        implicit
            .text
            .ends_with(r#"WHERE ("IsActiveEntity" = 0 OR "SiblingEntity/IsActiveEntity" IS NULL)"#),
        "{}",
        implicit.text
    );
    assert!(explicit
        .text
        .contains(r#""SiblingEntity/IsActiveEntity" IS NULL"#));
    Ok(())
}

/// Sets ohne Drafts bekommen weder Sibling-Join noch Draft-Flags -
/// die Spalte `IsActiveEntity` gibt es dort gar nicht.
#[test]
fn sets_without_drafts_skip_the_sibling_join() -> anyhow::Result<()> {
    let set = EntitySet::new("ThingSet", "Thing")
        .column(Column::new("name", DataType::String { max_length: None }));
    let s = Translator::new(&set, &SQLITE).build()?;
    assert_eq!(
        s.text,
        normalize(r#"SELECT * FROM (SELECT * FROM "ThingSet")"#)
    );
    Ok(())
}

/// Fehler muessen aus `build()` herauskommen, nicht als Panic.
#[test]
fn untranslatable_filters_return_an_error() {
    let set = order_set();
    for filter in ["floor(Amount) eq 1", "Nope eq 'x'", "Customer/Name eq 'x'"] {
        let expr = Expr::parse(filter).expect("parst");
        let result = Translator::new(&set, &SQLITE).filter(&expr).build();
        assert!(result.is_err(), "{filter} haette scheitern muessen");
    }
}
