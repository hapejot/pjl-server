//! Konsistenz zwischen `ddic.json`, Schema, Datenbank-DDL und OData-Modell.
//!
//! Alle Pruefungen laufen auf der `ddic.json` im Repo. Sie ist das Ergebnis
//! des Generators *nach* den Overrides aus `src/map.rs`. Die Quelldatenbank
//! wird dabei nicht gebraucht.

use data_store::{Cardinality, Ownership};
use relational_core::catalog::FakeCatalog;
use relational_core::ddl;
use spec::data_dictionary::DataDictionary;

fn schema() -> data_store::Schema {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/ddic.json");
    let text = std::fs::read_to_string(path).expect("ddic.json lesbar");
    let mut ddic: DataDictionary = serde_json::from_str(&text).expect("ddic.json parsbar");
    ddic.activate();
    spec_schema::schema_from_ddic(&ddic).expect("Schema baut")
}

/// Jede Komposition im Schema bekommt genau einen Fremdschluessel mit CASCADE,
/// jede Assoziation keinen.
#[test]
fn ddl_constraints_follow_ownership_of_ddic() {
    let schema = schema();
    for set in schema.sets() {
        let desired = ddl::desired_foreign_key(&schema, set).expect("FK-Bestimmung");
        let composition = set.composition_parent();
        assert_eq!(
            desired.is_some(),
            composition.is_some(),
            "{}: FK-Erwartung passt nicht zur Ownership",
            set.name
        );
        if let (Some(fk), Some(parent)) = (desired, composition) {
            assert_eq!(fk.target_table, parent.target_set, "{}", set.name);
            assert!(fk.clause.ends_with("ON DELETE CASCADE"), "{}", set.name);
        }
    }
}

/// Der Live-Fall: `VideoJobSet` hat einen CASCADE-Fremdschluessel auf
/// `agent_ID` aus der Zeit, als der Agent Komposition war. Der Abgleich
/// muss ihn entfernen.
#[tokio::test]
async fn video_job_agent_constraint_is_removed_when_agent_is_association() {
    let schema = schema();
    let set = schema.set("VideoJobSet").unwrap();
    assert!(set.composition_parent().is_none(), "VideoJob ist keine Komposition unter Agent");

    let catalog = FakeCatalog::default()
        .with_table("VideoJobSet", &["ID", "IsActiveEntity", "agent_ID"])
        .with_foreign_key(
            "VideoJobSet",
            "VideoJobSet_agent_ID_IsActiveEntity_fkey",
            &["agent_ID", "IsActiveEntity"],
            "AgentSet",
        );
    let plan = ddl::foreign_key_plan(&catalog, &schema).await.unwrap();
    assert_eq!(
        plan,
        vec![r#"ALTER TABLE "VideoJobSet" DROP CONSTRAINT "VideoJobSet_agent_ID_IsActiveEntity_fkey""#]
    );
}

/// Die Overrides aus `map.rs` sind in der Datei angekommen.
#[test]
fn override_results_are_in_ddic_json() {
    let schema = schema();
    let agent = schema.set("VideoJobSet").unwrap().associations().find(|a| a.name == "agent").unwrap();
    assert_eq!(agent.ownership, Ownership::Association);
    assert_eq!(agent.cardinality, Cardinality::ToOne);

    let video = schema.set("VideoSet").unwrap().associations().find(|a| a.name == "agent").unwrap();
    assert_eq!(video.ownership, Ownership::Association);

    // Overlay-Oberflaeche bleibt ein eigener Schritt, nicht Teil der ddic.
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/ddic.json")).unwrap();
    assert!(!text.contains("\"list_views\""), "Overlay-Ansichten gehoeren nicht in den Generator-Output");
}
