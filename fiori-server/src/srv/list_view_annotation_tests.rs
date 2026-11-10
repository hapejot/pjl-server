use super::*;
use spec::{FieldSpec, ListView, Relationship};

fn atom(name: &str) -> FieldSpec {
    FieldSpec::Atom {
        name: name.into(),
        label: name.into(),
        edm_type: "Edm.String".into(),
        package: None,
        max_length: None,
        precision: None,
        scale: None,
        computed: false,
        immutable: false,
        value_list: None,
        unit_scale_field: None,
        code_list: None,
        link: false,
        multiline: false,
        side_effects: Vec::new(),
        ui: None,
    }
}

#[test]
fn link_atoms_become_clickable_fields() {
    let mut e = active_video(vec![ListView::new("default", &["title"])]);
    e.update_fields(|f| {
        if let FieldSpec::Atom { name, link, .. } = f {
            if name == "title" {
                *link = true;
            }
        }
    });
    let fg = serde_json::to_string(&field_group(
        &DataDictionary::new(),
        "Main",
        &e,
        None,
        &[],
        &[],
    ))
    .unwrap();
    assert!(
        fg.contains("UI.DataFieldWithUrl"),
        "link-Atom wird DataFieldWithUrl"
    );
    let anns = line_item_annotations(&DataDictionary::new(), &e, &[]);
    let li = serde_json::to_string(&anns[0]).unwrap();
    assert!(li.contains("UI.DataFieldWithUrl"));
    assert!(
        !serde_json::to_string(&field_group(
            &DataDictionary::new(),
            "Main",
            &active_video(vec![]),
            None,
            &[],
            &[]
        ))
        .unwrap()
        .contains("DataFieldWithUrl")
    );
}

#[test]
fn header_type_names_are_display_texts_not_technical_names() {
    let mut e = active_video(vec![]);
    let header = |e: &spec::EntitySpec| {
        let anns = Annotations::from_entity(&DataDictionary::new(), e, &[], &[]);
        let main = anns.iter().find(|a| a.target == "Service.Video").unwrap();
        serde_json::to_string(
            main.annotations
                .iter()
                .find(|a| a.term == "UI.HeaderInfo")
                .unwrap(),
        )
        .unwrap()
    };
    let h = header(&e);
    assert!(h.contains("\"Videos\""), "Plural aus type_name_plural");
    assert!(!h.contains("VideoSet"));
    e.set_type_name(Some("Film".into()));
    assert!(header(&e).contains("\"Film\""), "TypeName aus type_name");
}

fn active_video(views: Vec<ListView>) -> spec::EntitySpec {
    let mut ddic = spec::data_dictionary::DataDictionary::empty();
    ddic.add_package(spec::package::Package::new("core"));
    ddic.add_entity(spec::EntitySpec::new("Agent", "core", "Agents").field(atom("name")));
    let mut video = spec::EntitySpec::new("Video", "core", "Videos")
        .field(atom("name"))
        .field(atom("title"));
    for v in views {
        video = video.list_view(v);
    }
    ddic.add_entity(video);
    ddic.add_relationship(
        Relationship::new("videos_agent_id", "core")
            .with_one("Agent", "_Videos")
            .with_many("Video", "agent"),
    );
    ddic.validate_list_views().expect("valid list views");
    ddic.activate();
    ddic.active_entity("Video").unwrap().clone()
}

fn find<'a>(anns: &'a [Annotation], term: &str, qualifier: Option<&str>) -> &'a Annotation {
    anns.iter()
        .find(|a| a.term == term && a.qualifier.as_deref() == qualifier)
        .unwrap_or_else(|| panic!("{term} qualifier={qualifier:?} fehlt"))
}

fn datafield_paths(a: &Annotation) -> Vec<String> {
    a.collection
        .as_ref()
        .unwrap()
        .records
        .iter()
        .map(|r| {
            r.property_values
                .iter()
                .find(|p| p.property == "Value")
                .and_then(|p| p.path.clone())
                .unwrap()
        })
        .collect()
}

#[test]
fn selection_fields_become_ui_selection_fields() {
    let mut e = active_video(vec![]);
    e.set_selection_fields(vec!["title".into(), "name".into()]);
    let anns = Annotations::from_entity(&DataDictionary::new(), &e, &[], &[]);
    let main = anns.iter().find(|a| a.target == "Service.Video").unwrap();
    let sel = main
        .annotations
        .iter()
        .find(|a| a.term == "UI.SelectionFields")
        .expect("UI.SelectionFields fehlt");
    assert_eq!(
        datafield_paths_of_property_paths(sel),
        vec!["title", "name"]
    );
}

fn datafield_paths_of_property_paths(a: &Annotation) -> Vec<String> {
    a.collection.as_ref().unwrap().property_paths.clone()
}

#[test]
fn filter_defaults_become_property_annotations() {
    let mut e = active_video(vec![]);
    e.set_filter_default("name", spec::FilterDefault::Text("Demo".into()));
    e.set_filter_default("title", spec::FilterDefault::Flag(true));
    let anns = Annotations::from_entity(&DataDictionary::new(), &e, &[], &[]);
    let on = |target: &str| {
        anns.iter()
            .filter(|a| a.target == target)
            .flat_map(|a| a.annotations.iter())
            .find(|a| a.term == "Common.FilterDefaultValue")
            .expect("FilterDefaultValue fehlt")
    };
    assert_eq!(on("Service.Video/name").string.as_deref(), Some("Demo"));
    assert_eq!(on("Service.Video/title").bool_, Some(true));
}

#[test]
fn general_facet_with_fields_restricts_the_main_group() {
    let mut e = active_video(vec![]);
    e.add_facet(spec::FacetSpec::new(
        "General",
        None,
        None,
        vec!["name".into()],
    ));
    let (_, groups) = object_page_sections(&DataDictionary::new(), &e, &[], &[]);
    let main = serde_json::to_string(&groups[0]).unwrap();
    assert!(main.contains("\"name\""));
    assert!(!main.contains("\"title\""));
}

#[test]
fn default_facet_collects_unmentioned_fields() {
    let mut e = active_video(vec![]);
    e.add_facet(spec::FacetSpec::new(
        "General",
        None,
        None,
        vec!["name".into()],
    ));
    e.add_facet(spec::FacetSpec::new(
        "Default",
        None,
        Some("Weitere".into()),
        vec![],
    ));
    let (records, groups) = object_page_sections(&DataDictionary::new(), &e, &[], &[]);
    let rest = groups
        .iter()
        .find(|g| g.qualifier.as_deref() == Some("Default"))
        .unwrap();
    let rest = serde_json::to_string(rest).unwrap();
    assert!(rest.contains("\"title\""));
    assert!(
        !rest.contains("\"name\""),
        "genannte Felder stehen nicht in Default"
    );
    let default_ref = serde_json::to_string(&records).unwrap();
    assert!(default_ref.contains("Weitere"));
}

#[test]
fn custom_facet_gets_its_own_field_group() {
    let mut e = active_video(vec![]);
    e.add_facet(spec::FacetSpec::new(
        "Meta",
        None,
        Some("Meta".into()),
        vec!["title".into()],
    ));
    let (_, groups) = object_page_sections(&DataDictionary::new(), &e, &[], &[]);
    let meta = groups
        .iter()
        .find(|g| g.qualifier.as_deref() == Some("Meta"))
        .unwrap();
    assert!(serde_json::to_string(meta).unwrap().contains("\"title\""));
    // Ohne General-Konfiguration bleibt der Hauptabschnitt mit allen Feldern erhalten.
    let main = groups
        .iter()
        .find(|g| g.qualifier.as_deref() == Some("Main"))
        .unwrap();
    assert!(serde_json::to_string(main).unwrap().contains("\"name\""));
}

#[test]
fn without_views_line_item_shows_name_only() {
    let e = active_video(vec![]);
    let anns = line_item_annotations(&DataDictionary::new(), &e, &[]);
    assert_eq!(anns.len(), 1);
    assert_eq!(anns[0].qualifier, None);
    assert_eq!(datafield_paths(&anns[0]), vec!["name"]);
}

#[test]
fn without_name_field_uses_first_displayable_field() {
    let entity = EntitySpec::new("ListColumn", "core", "List Columns")
        .field(FieldSpec::atom("field", "Field", "Edm.String"))
        .field(FieldSpec::atom("order", "Position", "Edm.Int64"));
    let annotations = line_item_annotations(&DataDictionary::new(), &entity, &[]);
    assert_eq!(datafield_paths(&annotations[0]), vec!["field"]);
}

#[test]
fn column_priority_becomes_importance_and_width_goes_to_config() {
    use spec::{ListColumn, Priority};
    let e = active_video(vec![ListView::with_columns(
        "default",
        vec![
            ListColumn::new("name")
                .with_width("20%")
                .with_priority(Priority::High),
            ListColumn::new("title"),
        ],
    )]);
    let anns = line_item_annotations(&DataDictionary::new(), &e, &[]);
    let default = find(&anns, "UI.LineItem", None);
    let records = &default.collection.as_ref().unwrap().records;
    assert_eq!(
        records[0].annotations[0].term, "UI.Importance",
        "Prioritaet am Datenfeld"
    );
    assert_eq!(
        records[0].annotations[0].enum_member.as_deref(),
        Some("UI.ImportanceType/High")
    );
    assert!(records[1].annotations.is_empty());

    let mut ddic = spec::data_dictionary::DataDictionary::empty();
    ddic.add_package(spec::package::Package::new("core"));
    ddic.add_entity(e);
    ddic.activate();
    let widths = list_column_widths(&ddic, "Video").expect("Breiten");
    assert_eq!(
        widths["@com.sap.vocabularies.UI.v1.LineItem"]["columns"]["DataField::name"]["width"],
        "20%"
    );
    assert!(
        widths["@com.sap.vocabularies.UI.v1.LineItem"]["columns"]
            .get("DataField::title")
            .is_none()
    );
}

#[test]
fn default_and_named_views_get_qualifiers_and_navigation_paths() {
    let e = active_video(vec![
        ListView::new("default", &["name", "agent", "title"]),
        ListView::new("review", &["title"]),
    ]);
    let anns = line_item_annotations(&DataDictionary::new(), &e, &[]);

    let default = find(&anns, "UI.LineItem", None);
    assert_eq!(
        datafield_paths(default),
        vec!["name", "agent/name", "title"]
    );

    let review = find(&anns, "UI.LineItem", Some("review"));
    assert_eq!(datafield_paths(review), vec!["title"]);

    let pv_default = find(&anns, "UI.PresentationVariant", None);
    let vis = pv_default
        .record
        .as_ref()
        .unwrap()
        .property_values
        .iter()
        .find(|p| p.property == "Visualizations")
        .unwrap()
        .collection
        .as_ref()
        .unwrap();
    assert_eq!(vis.annotation_paths, vec!["@UI.LineItem"]);

    let pv_review = find(&anns, "UI.PresentationVariant", Some("review"));
    let vis = pv_review
        .record
        .as_ref()
        .unwrap()
        .property_values
        .iter()
        .find(|p| p.property == "Visualizations")
        .unwrap()
        .collection
        .as_ref()
        .unwrap();
    assert_eq!(vis.annotation_paths, vec!["@UI.LineItem#review"]);
}

#[test]
fn views_settings_only_with_more_than_one_view() {
    let single = active_video(vec![ListView::new("default", &["name"])]);
    assert!(crate::flp::list_views_settings(&single, &[]).is_empty());

    let multi = active_video(vec![
        ListView::new("review", &["title"]),
        ListView::new("default", &["name"]),
    ]);
    let settings = crate::flp::list_views_settings(&multi, &[]);
    let paths = settings["views"]["paths"].as_array().unwrap();
    assert_eq!(paths[0]["key"], "default");
    assert_eq!(
        paths[1]["annotationPath"],
        "com.sap.vocabularies.UI.v1.PresentationVariant#review"
    );
}
