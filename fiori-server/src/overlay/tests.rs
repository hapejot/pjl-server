use super::*;
use spec::{EntitySpec, FieldSpec, ListView};

fn base() -> DataDictionary {
    let mut d = DataDictionary::empty();
    d.add_package(spec::package::Package::new("core"));
    d.add_entity(
        EntitySpec::new("Video", "core", "Videos")
            .field(FieldSpec::atom("name", "Name", "Edm.String"))
            .field(FieldSpec::atom("file_size", "Size", "Edm.Int64"))
            .list_view(ListView::new("default", &["name", "file_size"]))
            .list_view(ListView::new("review", &["name"])),
    );
    d
}

fn video(d: &DataDictionary) -> EntitySpec {
    d.entities()
        .into_iter()
        .find(|e| e.name() == "Video")
        .unwrap()
}

#[test]
fn draft_mode_can_be_overridden_in_both_directions() {
    let disabled = parse(r#"{"entities":[{"name":"Video","draft_enabled":false}]}"#).unwrap();
    let ddic = apply(base(), &disabled).unwrap();
    assert!(!video(&ddic).draft_enabled());
    let enabled = parse(r#"{"entities":[{"name":"Video","draft_enabled":true}]}"#).unwrap();
    assert!(video(&apply(ddic, &enabled).unwrap()).draft_enabled());
}

#[test]
fn list_views_merge_by_name() {
    let overlay = parse(
        r#"{"entities":[{"name":"Video","list_views":[
                {"name":"default","columns":["file_size"]},
                {"name":"archive","columns":["name","file_size"]}]}]}"#,
    )
    .unwrap();
    let d = apply(base(), &overlay).unwrap();
    let views = video(&d);
    let views = views.list_views();
    assert_eq!(views.len(), 3);
    let default = views.iter().find(|v| v.name() == "default").unwrap();
    assert_eq!(names(&default.columns()), vec!["file_size"]);
    let review = views.iter().find(|v| v.name() == "review").unwrap();
    assert_eq!(names(&review.columns()), vec!["name"]);
    assert!(views.iter().any(|v| v.name() == "archive"));
}

#[test]
fn list_view_columns_accept_width_and_priority() {
    let overlay = parse(
        r#"{"entities":[{"name":"Video","list_views":[
                {"name":"default","columns":[
                    {"field":"name","width":"20%","priority":"high"},
                    "file_size"]}]}]}"#,
    )
    .unwrap();
    let d = apply(base(), &overlay).unwrap();
    let views = video(&d);
    let default = views
        .list_views()
        .iter()
        .find(|v| v.name() == "default")
        .unwrap();
    assert_eq!(default.columns()[0].width(), Some("20%"));
    assert_eq!(default.columns()[0].priority(), Some(spec::Priority::High));
    assert_eq!(default.columns()[1].width(), None);
    assert_eq!(default.columns()[1].priority(), None);
}

fn names(columns: &[spec::ListColumn]) -> Vec<&str> {
    columns.iter().map(|c| c.field()).collect()
}

#[test]
fn filter_defaults_are_set_and_overridable() {
    let first =
        parse(r#"{"entities":[{"name":"Video","filter_defaults":{"name":"Demo"}}]}"#).unwrap();
    let d = apply(base(), &first).unwrap();
    assert_eq!(
        video(&d).filter_defaults()["name"],
        FilterDefault::Text("Demo".into())
    );

    let second =
        parse(r#"{"entities":[{"name":"Video","filter_defaults":{"name":"Neu"}}]}"#).unwrap();
    let d = apply(d, &second).unwrap();
    assert_eq!(
        video(&d).filter_defaults()["name"],
        FilterDefault::Text("Neu".into())
    );
    assert_eq!(video(&d).filter_defaults().len(), 1);
}

#[test]
fn filter_default_on_unknown_or_non_atom_field_is_an_error() {
    let unknown =
        parse(r#"{"entities":[{"name":"Video","filter_defaults":{"nope":"x"}}]}"#).unwrap();
    assert!(apply(base(), &unknown).is_err());
}

#[test]
fn filter_default_numbers_are_rejected() {
    assert!(parse(r#"{"entities":[{"name":"Video","filter_defaults":{"name":5}}]}"#).is_err());
}

#[test]
fn selection_fields_are_ordered_and_unique() {
    let o = parse(r#"{"entities":[{"name":"Video","selection_fields":["name","file_size"]}]}"#)
        .unwrap();
    let d = apply(base(), &o).unwrap();
    let d = apply(
        d,
        &parse(r#"{"entities":[{"name":"Video","selection_fields":["file_size"]}]}"#).unwrap(),
    )
    .unwrap();
    assert_eq!(
        video(&d).selection_fields(),
        &["name".to_string(), "file_size".to_string()]
    );
}

#[test]
fn selection_fields_accept_to_one_navigation() {
    let mut base = base();
    base.add_entity(
        EntitySpec::new("Agent", "core", "Agents").field(FieldSpec::atom(
            "name",
            "Name",
            "Edm.String",
        )),
    );
    base.add_relationship(
        spec::Relationship::new("videos_agent", "core")
            .with_one("Agent", "_Videos")
            .with_many("Video", "agent"),
    );
    let o = parse(r#"{"entities":[{"name":"Video","selection_fields":["agent"]}]}"#).unwrap();
    assert!(apply(base, &o).is_ok());
}

#[test]
fn selection_field_must_be_atom() {
    let o = parse(r#"{"entities":[{"name":"Video","selection_fields":["gibts_nicht"]}]}"#).unwrap();
    assert!(apply(base(), &o).is_err());
}

#[test]
fn scalars_override_the_base() {
    let overlay = parse(r#"{"entities":[{"name":"Video","type_name_plural":"Filme"}]}"#).unwrap();
    let d = apply(base(), &overlay).unwrap();
    assert_eq!(video(&d).type_name_plural(), "Filme");
}

#[test]
fn unknown_entity_is_an_error() {
    let overlay = parse(r#"{"entities":[{"name":"Nope"}]}"#).unwrap();
    let err = apply(base(), &overlay).unwrap_err().to_string();
    assert!(err.contains("Nope"), "{err}");
}

#[test]
fn unknown_keys_are_rejected() {
    // Tippfehler duerfen nicht still verschluckt werden.
    assert!(parse(r#"{"entities":[{"name":"Video","list_view":[]}]}"#).is_err());
    assert!(parse(r#"{"entities":[{"name":"Video","fields":[]}]}"#).is_err());
}

fn base_with_agent() -> DataDictionary {
    let mut d = base();
    d.add_entity(
        EntitySpec::new("Agent", "core", "Agents").field(FieldSpec::atom(
            "name",
            "Name",
            "Edm.String",
        )),
    );
    d.add_relationship(
        spec::Relationship::new("videos_agent", "core")
            .with_one("Agent", "_Videos")
            .with_many("Video", "agent"),
    );
    d
}

#[test]
fn navigation_labels_come_from_the_relationship() {
    let mut d = base();
    d.add_entity(
        EntitySpec::new("Agent", "core", "Agents").field(FieldSpec::atom(
            "name",
            "Name",
            "Edm.String",
        )),
    );
    let mut rel = spec::Relationship::new("videos_agent", "core")
        .with_one("Agent", "_Videos")
        .with_many("Video", "agent");
    rel.set_one_label(Some("Videos".into()));
    rel.set_many_label(Some("Agent".into()));
    d.add_relationship(rel);
    d.activate();
    let agent = d.active_entity("Agent").unwrap();
    assert_eq!(
        agent
            .fields()
            .find(|f| f.name() == "_Videos")
            .unwrap()
            .label(),
        "Videos"
    );
    let video = d.active_entity("Video").unwrap();
    assert_eq!(
        video
            .fields()
            .find(|f| f.name() == "agent")
            .unwrap()
            .label(),
        "Agent"
    );
}

#[test]
fn facet_is_merged_and_kept() {
    let o =
        parse(r#"{"entities":[{"name":"Agent","facets":[{"facet":"_Videos","view":"review"}]}]}"#)
            .unwrap();
    let d = apply(base_with_agent(), &o).unwrap();
    let agent = d
        .entities()
        .into_iter()
        .find(|e| e.name() == "Agent")
        .unwrap();
    assert_eq!(
        agent.facet("_Videos").unwrap().view().as_deref(),
        Some("review")
    );
}

#[test]
fn facet_unknown_view_is_an_error() {
    let o = parse(
        r#"{"entities":[{"name":"Agent","facets":[{"facet":"_Videos","view":"gibts_nicht"}]}]}"#,
    )
    .unwrap();
    let err = apply(base_with_agent(), &o).unwrap_err().to_string();
    assert!(err.contains("gibts_nicht"), "{err}");
}

#[test]
fn facet_default_is_always_valid() {
    let o =
        parse(r#"{"entities":[{"name":"Agent","facets":[{"facet":"_Videos","view":"default"}]}]}"#)
            .unwrap();
    assert!(apply(base_with_agent(), &o).is_ok());
}

#[test]
fn facet_title_is_merged_and_blank_is_rejected() {
    let o =
        parse(r#"{"entities":[{"name":"Agent","facets":[{"facet":"_Videos","title":"Filme"}]}]}"#)
            .unwrap();
    let d = apply(base_with_agent(), &o).unwrap();
    let agent = d
        .entities()
        .into_iter()
        .find(|e| e.name() == "Agent")
        .unwrap();
    assert_eq!(
        agent.facet("_Videos").unwrap().title().as_deref(),
        Some("Filme")
    );

    let blank =
        parse(r#"{"entities":[{"name":"Agent","facets":[{"facet":"_Videos","title":"  "}]}]}"#)
            .unwrap();
    assert!(apply(base_with_agent(), &blank).is_err());
}

#[test]
fn general_facet_takes_title_but_no_view() {
    let o = parse(
        r#"{"entities":[{"name":"Agent","facets":[{"facet":"General","title":"Stammdaten"}]}]}"#,
    )
    .unwrap();
    let d = apply(base_with_agent(), &o).unwrap();
    let agent = d
        .entities()
        .into_iter()
        .find(|e| e.name() == "Agent")
        .unwrap();
    assert_eq!(
        agent.facet("General").unwrap().title().as_deref(),
        Some("Stammdaten")
    );

    let with_view =
        parse(r#"{"entities":[{"name":"Agent","facets":[{"facet":"General","view":"review"}]}]}"#)
            .unwrap();
    assert!(apply(base_with_agent(), &with_view).is_err());
}

#[test]
fn facet_navigation_must_be_to_many() {
    let o = parse(r#"{"entities":[{"name":"Video","facets":[{"facet":"agent"}]}]}"#).unwrap();
    assert!(apply(base_with_agent(), &o).is_err());
    let o = parse(r#"{"entities":[{"name":"Agent","facets":[{"facet":"nope","view":"review"}]}]}"#)
        .unwrap();
    assert!(apply(base_with_agent(), &o).is_err());
}

#[test]
fn empty_overlay_keeps_the_base() {
    let d = apply(base(), &parse("{}").unwrap()).unwrap();
    assert_eq!(video(&d).list_views().len(), 2);
}
