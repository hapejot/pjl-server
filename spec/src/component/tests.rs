use super::*;

fn comp() -> ComponentSpec {
    ComponentSpec {
        name: "TimeRange".into(),
        kind: ComponentKind::TimeRange,
        script: Some("components/TimeRange".into()),
        unit: TimeUnit::Minutes,
        step_minutes: 5,
    }
}

fn usage(instance: &str, start: &str, end: &str, dur: &str) -> ComponentUse {
    ComponentUse {
        entity: "Appointment".into(),
        component: "TimeRange".into(),
        instance: instance.into(),
        fields: BTreeMap::from([
            ("start".into(), start.into()),
            ("end".into(), end.into()),
            ("duration".into(), dur.into()),
        ]),
        labels: BTreeMap::new(),
        step_minutes: None,
        section_title: Some("Zeitraum".into()),
    }
}

fn appointment() -> EntitySpec {
    serde_json::from_value(serde_json::json!({
        "name": "Appointment",
        "package": "workbench",
        "type_name_plural": "Termine",
        "fields": [ {"Atom": {"name": "name", "label": "Name", "edm_type": "Edm.String"}} ]
    }))
    .unwrap()
}

#[test]
fn expands_fields_side_effects_and_hidden_fields() {
    let mut entities = vec![appointment()];
    expand(
        &mut entities,
        &[comp()],
        &[usage("termin", "starts_at", "ends_at", "duration_minutes")],
    )
    .unwrap();
    let e = &entities[0];
    assert!(e.fields().any(|f| f.name() == "starts_at"));
    let FieldSpec::Atom {
        edm_type,
        side_effects,
        label,
        ..
    } = e.fields().find(|f| f.name() == "duration_minutes").unwrap()
    else {
        panic!("kein Atom");
    };
    assert_eq!(edm_type, "Edm.Int32");
    assert_eq!(label, "Dauer (Minuten)");
    assert_eq!(
        side_effects,
        &vec!["starts_at".to_string(), "ends_at".to_string()]
    );
    let hidden = &e.ui.as_ref().unwrap().hidden_fields;
    assert_eq!(
        hidden,
        &vec![
            "starts_at".to_string(),
            "ends_at".to_string(),
            "duration_minutes".to_string()
        ]
    );
}

#[test]
fn expansion_is_idempotent() {
    let mut entities = vec![appointment()];
    let u = [usage("termin", "starts_at", "ends_at", "duration_minutes")];
    expand(&mut entities, &[comp()], &u).unwrap();
    let once = serde_json::to_string(&entities).unwrap();
    expand(&mut entities, &[comp()], &u).unwrap();
    assert_eq!(once, serde_json::to_string(&entities).unwrap());
}

#[test]
fn two_instances_in_one_entity_do_not_clash() {
    let mut entities = vec![appointment()];
    let uses = [
        usage("termin", "starts_at", "ends_at", "duration_minutes"),
        usage("ist", "actual_start", "actual_end", "actual_minutes"),
    ];
    expand(&mut entities, &[comp()], &uses).unwrap();
    assert!(entities[0].fields().any(|f| f.name() == "actual_start"));
    assert!(entities[0].fields().any(|f| f.name() == "starts_at"));
}

#[test]
fn rejects_existing_field_with_other_type() {
    let mut entities = vec![appointment()];
    let mut u = usage("termin", "name", "ends_at", "duration_minutes");
    u.fields.insert("start".into(), "name".into());
    let err = expand(&mut entities, &[comp()], &[u]).unwrap_err();
    assert!(err.contains("Typ"), "{err}");
}

#[test]
fn rejects_bad_step_and_missing_role() {
    let mut c = comp();
    c.step_minutes = 7;
    assert!(validate(&[appointment()], &[c], &[]).is_err());

    let mut u = usage("termin", "a", "b", "c");
    u.fields.remove("duration");
    assert!(validate(&[appointment()], &[comp()], &[u]).is_err());
}

#[test]
fn step_override_per_use() {
    let mut u = usage("termin", "a", "b", "c");
    assert_eq!(effective_step(&comp(), &u), 5);
    u.step_minutes = Some(15);
    assert_eq!(effective_step(&comp(), &u), 15);
}
