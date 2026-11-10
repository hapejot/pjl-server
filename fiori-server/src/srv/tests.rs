use super::*;

#[test]
fn entity_type_from_entity_maps_field_variants() {
    let mut ddic = DataDictionary::new();
    ddic.activate();
    let entity = ddic.active_entity("Entity").unwrap();

    let entity_type = EntityType::from_entity(&ddic, entity);
    dbg!(&entity_type);
    assert!(
        entity_type
            .properties
            .iter()
            .any(|property| property.name == "name" && property.type_ == "Edm.String")
    );
}

#[test]
fn lookup_to_app_entity_gets_intent_based_navigation() {
    let mut ddic = DataDictionary::new();
    ddic.activate();
    let (entity, target, name) = ddic
        .active_entities()
        .find_map(|e| {
            e.fields().find_map(|f| match f {
                FieldSpec::Lookup {
                    name,
                    entity,
                    collection: false,
                    ..
                } => Some((e.clone(), entity.clone(), name.clone())),
                _ => None,
            })
        })
        .expect("ein Einzel-Lookup im Standard-Dictionary");
    let fk = format!("{name}_ID");

    let linked = Annotations::from_entity(
        &DataDictionary::new(),
        &entity,
        &[],
        std::slice::from_ref(&target),
    );
    let xml = format!("{linked:?}");
    assert!(xml.contains("UI.DataFieldWithIntentBasedNavigation"));
    assert!(xml.contains(&target));

    let plain = Annotations::from_entity(&DataDictionary::new(), &entity, &[], &[]);
    let xml = format!("{plain:?}");
    assert!(!xml.contains("UI.DataFieldWithIntentBasedNavigation"));
    assert!(xml.contains(&fk));
}

#[test]
fn facet_target_uses_the_configured_view() {
    let mut x = EntitySpec::new("Actor", "core", "Actors");
    assert_eq!(
        facet_target_path(&x, "_ActorSceneMap"),
        "_ActorSceneMap/@UI.LineItem"
    );
    x.add_facet(spec::FacetSpec::new(
        "_ActorSceneMap",
        Some("reverse".into()),
        None,
        vec![],
    ));
    assert_eq!(
        facet_target_path(&x, "_ActorSceneMap"),
        "_ActorSceneMap/@UI.LineItem#reverse"
    );
    x.set_facets(vec![spec::FacetSpec::new(
        "_ActorSceneMap",
        Some("default".into()),
        None,
        vec![],
    )]);
    assert_eq!(
        facet_target_path(&x, "_ActorSceneMap"),
        "_ActorSceneMap/@UI.LineItem"
    );
}

#[test]
fn facet_title_prefers_the_model_label_of_the_navigation() {
    let mut x = EntitySpec::new("Actor", "core", "Actors");
    x.add_field(FieldSpec::Lookup {
        name: "_ActorSceneMap".into(),
        entity: "ActorSceneMap".into(),
        collection: true,
        owned: false,
        label: Some("Scene assignments".into()),
    });
    assert_eq!(facet_title(&x, "_ActorSceneMap"), "Scene assignments");
    x.add_facet(spec::FacetSpec::new(
        "_ActorSceneMap",
        None,
        Some("Szenen".into()),
        vec![],
    ));
    assert_eq!(facet_title(&x, "_ActorSceneMap"), "Szenen");
}

#[test]
fn facet_title_defaults_to_the_navigation_name() {
    let mut x = EntitySpec::new("Actor", "core", "Actors");
    assert_eq!(facet_title(&x, "_ActorSceneMap"), "_ActorSceneMap");
    x.add_facet(spec::FacetSpec::new(
        "_ActorSceneMap",
        None,
        Some("Szenen".into()),
        vec![],
    ));
    assert_eq!(facet_title(&x, "_ActorSceneMap"), "Szenen");
}

#[test]
fn facets_include_a_list_section_per_to_many_navigation() {
    let mut ddic = DataDictionary::new();
    ddic.activate();
    // "Entity" has to-many lookups _Fields, _Many, _One after activation.
    let entity = ddic.active_entity("Entity").unwrap();

    let annotations = Annotations::from_entity(&DataDictionary::new(), entity, &[], &[]);
    let main = annotations
        .iter()
        .find(|a| a.target == "Service.Entity")
        .unwrap();
    let facets = main
        .annotations
        .iter()
        .find(|a| a.term == "UI.Facets")
        .unwrap()
        .collection
        .as_ref()
        .unwrap();

    for nav_name in ["_Fields", "_Many", "_One"] {
        let record = facets
            .records
            .iter()
            .find(|r| {
                r.property_values.iter().any(|pv| {
                    pv.property == "ID"
                        && pv.string.as_deref() == Some(&format!("{nav_name}Section"))
                })
            })
            .unwrap_or_else(|| panic!("no list facet generated for {nav_name}"));
        let target = record
            .property_values
            .iter()
            .find(|pv| pv.property == "Target")
            .unwrap();
        assert_eq!(
            target.annotation_path.as_deref(),
            Some(format!("{nav_name}/@UI.LineItem").as_str())
        );
    }
}

#[test]
fn field_annotations_cover_atom_value_list_and_measure() {
    let value_list_field = FieldSpec::string("code", "Code", 20)
        .computed()
        .immutable()
        .with_value_list(AtomValueList::entity_ref("PackageSet", "ID", Some("name")));
    let value_list_annotations =
        Annotations::from_field(&DataDictionary::new(), "Invoice", &value_list_field);

    assert!(value_list_annotations.annotations.iter().any(|annotation| {
        annotation.term == "Common.Label" && annotation.string.as_deref() == Some("Code")
    }));
    assert!(
        value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Org.OData.Core.V1.Computed")
    );
    assert!(
        value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Org.OData.Core.V1.Immutable")
    );
    assert!(
        value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Common.ValueList")
    );
    assert!(
        value_list_annotations
            .annotations
            .iter()
            .any(|annotation| annotation.term == "Common.ValueListWithFixedValues")
    );

    let measure = FieldSpec::Measure {
        name: "amount".into(),
        label: "Amount".into(),
        package: None,
        precision: Some(15),
        scale: Some(2),
        unit_field: "currency".into(),
        kind: MeasureKind::Currency,
    };
    let measure_annotations = Annotations::from_field(&DataDictionary::new(), "Invoice", &measure);

    assert!(measure_annotations.annotations.iter().any(|annotation| {
        annotation.term == "Org.OData.Measures.V1.ISOCurrency"
            && annotation.path.as_deref() == Some("currency")
    }));
}

#[test]
fn measure_fields_render_as_data_field_and_unit_annotation() {
    for kind in [MeasureKind::Currency, MeasureKind::Unit] {
        let measure = FieldSpec::Measure {
            name: "amount".into(),
            label: "Amount".into(),
            package: None,
            precision: None,
            scale: None,
            unit_field: "unit".into(),
            kind,
        };
        let rec =
            field_record(&DataDictionary::new(), &measure, &[], &[]).expect("Measure ist sichtbar");
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("UI.DataField"));
        assert!(json.contains("amount"));
        assert!(
            field_record(
                &DataDictionary::new(),
                &measure,
                &["amount".to_string()],
                &[]
            )
            .is_none()
        );
    }
}

#[test]
fn parses_simple_expand_list() {
    let expand = parse_expand_items("DraftAdministrativeData,package").unwrap();

    assert_eq!(expand.len(), 2);
    assert_eq!(expand[0].navigation_property, "DraftAdministrativeData");
    assert_eq!(expand[1].navigation_property, "package");
}

#[test]
fn parses_expand_with_select_options() {
    let expand =
        parse_expand_items("DraftAdministrativeData($select=DraftUUID,InProcessByUser)").unwrap();

    assert_eq!(expand.len(), 1);
    assert_eq!(expand[0].navigation_property, "DraftAdministrativeData");
    assert_eq!(
        expand[0].nested_query.select,
        vec!["DraftUUID".to_string(), "InProcessByUser".to_string()]
    );
}

#[test]
fn parses_nested_expand_options() {
    let expand = parse_expand_items("_Entities($expand=package($select=name))").unwrap();

    assert_eq!(expand.len(), 1);
    assert_eq!(expand[0].navigation_property, "_Entities");
    assert_eq!(expand[0].nested_query.expand.len(), 1);
    assert_eq!(
        expand[0].nested_query.expand[0].navigation_property,
        "package"
    );
    assert_eq!(
        expand[0].nested_query.expand[0].nested_query.select,
        vec!["name".to_string()]
    );
}

#[test]
fn parses_orderby_list_with_directions() {
    let items = parse_orderby("ID,Amount desc, name ASC").unwrap();

    assert_eq!(items.len(), 3);
    assert_eq!(items[0].property, "ID");
    assert_eq!(items[0].direction, SortDirection::Asc);
    assert_eq!(items[1].property, "Amount");
    assert_eq!(items[1].direction, SortDirection::Desc);
    assert_eq!(items[2].property, "name");
    assert_eq!(items[2].direction, SortDirection::Asc);
}

#[test]
fn invalid_orderby_direction_is_bad_request() {
    assert!(parse_orderby("Amount sideways").is_err());
    assert!(parse_orderby("Amount desc extra").is_err());
}

#[test]
fn build_query_passes_orderby_through() {
    let mut params = HashMap::new();
    params.insert("$orderby".to_string(), "ID desc".to_string());
    let query = build_query(&params).unwrap();

    assert_eq!(query.orderby.len(), 1);
    assert_eq!(query.orderby[0].property, "ID");
    assert_eq!(query.orderby[0].direction, SortDirection::Desc);
}

#[test]
fn expand_option_orderby_is_parsed() {
    let expand = parse_expand_items("_Entities($orderby=name desc)").unwrap();

    assert_eq!(expand[0].nested_query.orderby.len(), 1);
    assert_eq!(expand[0].nested_query.orderby[0].property, "name");
    assert_eq!(
        expand[0].nested_query.orderby[0].direction,
        SortDirection::Desc
    );
}
