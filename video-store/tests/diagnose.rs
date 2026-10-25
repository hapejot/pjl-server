//! Diagnose fuer den Anlage-Pfad von `VideoJob`. Prueft gegen die `ddic.json`
//! im Repo, ob die Spalte `agent_ID` existiert, als Navigation `agent` gemappt
//! ist und die Payload des POST durch die Schluesselpruefung kommt.
//!
//! Standardmaessig ignoriert (braucht keine Datenbank, ist aber ein Werkzeug,
//! kein Regressionstest). Ausfuehren:
//!
//!   cargo test -p video-store --test diagnose -- --ignored --nocapture
//!
//! Optional gegen einen laufenden Dienst (prueft das `$metadata` des Dienstes,
//! also das, was der Client wirklich sieht):
//!
//!   VIDEO_STORE_URL=http://localhost:8080/odata/video \
//!     cargo test -p video-store --test diagnose -- --ignored --nocapture

use data_store::{Cardinality, Ownership};
use spec::data_dictionary::DataDictionary;

const SET: &str = "VideoJobSet";
const PAYLOAD: &[&str] = &["name", "job_type", "video_ID", "agent_ID", "IsActiveEntity"];

fn report(ok: bool, msg: &str) {
    println!("[{}] {msg}", if ok { " OK " } else { "FAIL" });
}

fn ddic_file() -> DataDictionary {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/ddic.json");
    let text = std::fs::read_to_string(path).expect("ddic.json lesbar");
    serde_json::from_str(&text).expect("ddic.json parsbar")
}

#[test]
#[ignore = "Diagnose: cargo test -p video-store --test diagnose -- --ignored --nocapture"]
fn video_job_agent_id_is_mapped() {
    let mut ddic = ddic_file();
    ddic.activate();
    let schema = spec_schema::schema_from_ddic(&ddic).expect("Schema aus ddic.json baut");

    println!("\n== 1. Schema aus ddic.json ({SET}) ==");
    let Ok(set) = schema.set(SET) else {
        report(false, &format!("{SET} existiert nicht im Schema"));
        panic!("{SET} fehlt");
    };
    report(true, &format!("{SET} existiert"));

    let has_col = set.has_column("agent_ID");
    report(has_col, "Spalte agent_ID vorhanden");

    let agent = set.associations().find(|a| a.name == "agent");
    match agent {
        Some(a) => {
            report(a.cardinality == Cardinality::ToOne, "Navigation agent ist ToOne");
            report(a.target_set == "AgentSet", &format!("agent zeigt auf {}", a.target_set));
            report(
                a.ownership == Ownership::Composition,
                &format!("agent ist Komposition ({:?})", a.ownership),
            );
        }
        None => report(false, "Navigation agent fehlt"),
    }

    println!("\n== 2. Payload-Pruefung wie im Store (postgres-relational-store) ==");
    let mut all_ok = true;
    for field in PAYLOAD {
        let known = set.has_column(field)
            || matches!(*field, "ID" | "IsActiveEntity")
            || set.get_association(field).is_ok();
        all_ok &= known;
        report(known, &format!("Feld '{field}' bekannt"));
    }

    assert!(has_col, "agent_ID fehlt im Schema");
    assert!(all_ok, "Payload enthaelt unbekannte Felder");
}

#[tokio::test]
#[ignore = "Diagnose gegen laufenden Dienst: VIDEO_STORE_URL setzen"]
async fn running_service_metadata_exposes_agent() {
    let Ok(base) = std::env::var("VIDEO_STORE_URL") else {
        println!("VIDEO_STORE_URL nicht gesetzt - uebersprungen");
        return;
    };
    let url = format!("{}/$metadata", base.trim_end_matches('/'));
    println!("\n== 3. Laufender Dienst: {url} ==");
    let body = reqwest::get(&url)
        .await
        .expect("Dienst erreichbar")
        .text()
        .await
        .expect("Body lesbar");

    let entity = body
        .find("EntityType Name=\"VideoJob\"")
        .map(|start| {
            let rest = &body[start..];
            let end = rest.find("</EntityType>").unwrap_or(rest.len());
            rest[..end].to_string()
        });

    match entity {
        Some(e) => {
            report(true, "EntityType VideoJob im $metadata");
            let nav = e.contains("NavigationProperty Name=\"agent\"");
            report(nav, "Navigation 'agent' im $metadata");
            let fk = e.contains("agent_ID");
            report(fk, "Eigenschaft agent_ID im $metadata");
            assert!(nav, "agent fehlt im $metadata des laufenden Dienstes");
        }
        None => {
            report(false, "EntityType VideoJob fehlt im $metadata");
            panic!("VideoJob fehlt im $metadata");
        }
    }
}
