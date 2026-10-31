//! Die Workbench-DDIC muss sich laden und expandieren lassen; die Termin-Felder
//! kommen dann aus der Komponenten-Verwendung.

use spec::{data_dictionary::DataDictionary, FieldSpec};

fn workbench() -> DataDictionary {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../workbench/ddic.json");
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).expect("workbench ddic parst")
}

#[test]
fn workbench_appointment_is_expanded_from_time_range() {
    let mut ddic = workbench();
    ddic.expand_components().expect("expandiert");

    let appt = ddic.entities().into_iter().find(|e| e.name == "Appointment").unwrap();
    let field = |n: &str| appt.fields.iter().find(|f| f.name() == n).unwrap().clone();

    let FieldSpec::Atom { edm_type, side_effects, label, .. } = field("starts_at") else {
        panic!("starts_at ist kein Atom");
    };
    assert_eq!(edm_type, "Edm.DateTimeOffset");
    assert_eq!(label, "Beginn");
    assert_eq!(side_effects, vec!["ends_at".to_string(), "duration_minutes".to_string()]);

    let FieldSpec::Atom { edm_type, .. } = field("duration_minutes") else {
        panic!("duration_minutes ist kein Atom");
    };
    assert_eq!(edm_type, "Edm.Int32");

    let hidden = &appt.ui.as_ref().unwrap().hidden_fields;
    assert!(hidden.contains(&"starts_at".to_string()));
    assert!(hidden.contains(&"duration_minutes".to_string()));
}

#[test]
fn expansion_twice_is_stable() {
    let mut ddic = workbench();
    ddic.expand_components().unwrap();
    let first = serde_json::to_string(&ddic.entities()).unwrap();
    ddic.expand_components().unwrap();
    assert_eq!(first, serde_json::to_string(&ddic.entities()).unwrap());
}
