use super::*;

fn dd_with(entity: EntitySpec) -> DataDictionary {
    let mut dd = DataDictionary::new();
    dd.entities.push(entity);
    dd
}

fn code_entity() -> EntitySpec {
    EntitySpec::new("Code", "core", "Codes")
        .without_drafts()
        .without_audit()
        .field(FieldSpec::string("code", "Code", 10))
        .field(FieldSpec::atom("name", "Bezeichnung", "Edm.String"))
        .with_primary_key("code")
}

#[test]
fn valid_string_key_is_accepted() {
    let dd = dd_with(code_entity());
    assert!(dd.validate_primary_keys().is_ok());
    let e = code_entity();
    assert_eq!(e.primary_key_name(), "code");
    assert_eq!(e.primary_key_type(), "Edm.String");
}

#[test]
fn default_key_is_id_guid() {
    let e = EntitySpec::new("X", "core", "Xs");
    assert_eq!(e.primary_key_name(), "ID");
    assert_eq!(e.primary_key_type(), "Edm.Guid");
}

#[test]
fn key_with_drafts_is_rejected() {
    let dd = dd_with(code_entity().field(FieldSpec::atom("x", "X", "Edm.String")));
    let mut dd = dd;
    dd.entities.last_mut().unwrap().draft_enabled = true;
    assert!(
        dd.validate_primary_keys()
            .unwrap_err()
            .contains("ohne Drafts")
    );
}

#[test]
fn missing_or_wrong_field_is_rejected() {
    let dd = dd_with(code_entity().with_primary_key("nope"));
    assert!(
        dd.validate_primary_keys()
            .unwrap_err()
            .contains("existiert nicht")
    );

    let dd = dd_with(
        code_entity()
            .with_primary_key("name")
            .field(FieldSpec::int("n", "N")),
    );
    assert!(dd.validate_primary_keys().is_ok());
}

#[test]
fn computed_or_value_list_key_is_rejected() {
    let e = EntitySpec::new("Code", "core", "Codes")
        .without_drafts()
        .field(FieldSpec::string("code", "Code", 10).computed())
        .with_primary_key("code");
    assert!(
        dd_with(e)
            .validate_primary_keys()
            .unwrap_err()
            .contains("berechnet")
    );
}

#[test]
fn unsupported_type_is_rejected() {
    let e = EntitySpec::new("Code", "core", "Codes")
        .without_drafts()
        .field(FieldSpec::atom("d", "D", "Edm.DateTimeOffset"))
        .with_primary_key("d");
    assert!(
        dd_with(e)
            .validate_primary_keys()
            .unwrap_err()
            .contains("nicht als Schluessel")
    );
}
