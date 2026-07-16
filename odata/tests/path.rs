use odata::path::*;

#[test]
fn parses_simple_segments() {
    let path = parse_odata_path("EntityConfigs/Fields").unwrap();
    assert_eq!(
        path.segments,
        vec![
            PathSegment {
                name: "EntityConfigs".into(),
                keys: vec![]
            },
            PathSegment {
                name: "Fields".into(),
                keys: vec![]
            },
        ]
    );
}

#[test]
fn parses_single_unnamed_key() {
    let path = parse_odata_path("EntityConfigs('abc')").unwrap();
    assert_eq!(
        path.segments,
        vec![PathSegment {
            name: "EntityConfigs".into(),
            keys: vec![KeyValue {
                name: None,
                value: KeyLiteral::String("abc".into())
            }],
        }]
    );
}

#[test]
fn parses_guid_key() {
    let path = parse_odata_path("EntityConfigs(12345678-1234-1234-1234-123456789012)").unwrap();
    assert_eq!(
        path.segments[0].keys[0].value,
        KeyLiteral::Guid("12345678-1234-1234-1234-123456789012".into())
    );
}

#[test]
fn parses_composite_named_keys() {
    let path = parse_odata_path(
        "EntityConfigs(ID=12345678-1234-1234-1234-123456789012,IsActiveEntity=true)",
    )
    .unwrap();

    assert_eq!(
        path.segments[0].keys,
        vec![
            KeyValue {
                name: Some("ID".into()),
                value: KeyLiteral::Guid("12345678-1234-1234-1234-123456789012".into()),
            },
            KeyValue {
                name: Some("IsActiveEntity".into()),
                value: KeyLiteral::Bool(true),
            },
        ]
    );
}

#[test]
fn parses_navigation_after_key() {
    let path = parse_odata_path("EntityConfigs('abc')/Fields(1)/$count").unwrap();
    assert_eq!(path.segments.len(), 3);
    assert_eq!(path.segments[1].name, "Fields");
    assert_eq!(path.segments[2].name, "$count");
}

#[test]
fn parses_leading_trailing_slashes() {
    let path = parse_odata_path("/EntityConfigs/").unwrap();
    assert_eq!(path.segments.len(), 1);
}

#[test]
fn parses_empty_path() {
    let path = parse_odata_path("").unwrap();
    assert!(path.segments.is_empty());
}

#[test]
fn error_on_invalid_key() {
    let result = parse_odata_path("EntityConfigs(not_a_valid_key!!)");
    assert!(result.is_err());
}
