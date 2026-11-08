use super::*;
use serde_json::json;

fn ddic_with_video() -> DataDictionary {
    let video: spec::EntitySpec = serde_json::from_value(json!({
        "name": "Video",
        "package": "video",
        "type_name_plural": "Videos",
        "fields": [
            {"Atom": {"name": "title", "label": "Titel", "edm_type": "Edm.String"}},
            {"Atom": {"name": "duration_seconds", "label": "Dauer", "edm_type": "Edm.Int32",
                      "ui": {"format": "duration"}}},
            {"Atom": {"name": "file_size", "label": "Groesse", "edm_type": "Edm.Int64",
                      "ui": {"format": "bytes"}}},
            {"Atom": {"name": "secret", "label": "Geheim", "edm_type": "Edm.String"}}
        ],
        "ui": {"hidden_fields": ["secret"]}
    }))
    .unwrap();
    let mut ddic = DataDictionary::empty();
    ddic.add_entity(video);
    ddic
}

#[test]
fn formatted_fields_become_list_columns_and_hide_raw_columns() {
    let ext = UiExtension::new(&ddic_with_video(), None);
    let cols = ext.list_columns("Video").expect("Spalten");
    assert_eq!(
        cols["Fmtduration_seconds"]["template"],
        "fiori.ext.fragment.duration_seconds__duration"
    );
    assert_eq!(
        cols["Fmtfile_size"]["template"],
        "fiori.ext.fragment.file_size__bytes"
    );
    assert_eq!(cols["DataField::duration_seconds"]["visible"], false);
    assert!(ext.list_columns("Other").is_none());
}

#[test]
fn object_page_section_lists_formatted_fields() {
    let ext = UiExtension::new(&ddic_with_video(), None);
    let page = ext.object_page_content("Video").expect("Abschnitt");
    assert_eq!(
        page["body"]["sections"]["FormattedFields"]["template"],
        "fiori.ext.fragment.Formatted__Video"
    );
    let fields = ext.formatted.get("Video").unwrap();
    let xml = section_fragment(fields);
    assert!(xml.contains("formatter: 'fiori.ext.Format.duration'"));
    assert!(xml.contains("formatter: 'fiori.ext.Format.bytes'"));
    assert!(xml.contains("<Label text=\"Dauer\"/>"));
}

#[test]
fn hidden_fields_come_from_entity_ui_block() {
    let ext = UiExtension::new(&ddic_with_video(), None);
    let hidden = ext.hidden_fields("Video");
    assert!(hidden.contains(&"secret".to_string()));
    assert!(hidden.contains(&"duration_seconds".to_string()));
    assert!(hidden.contains(&"file_size".to_string()));
    assert!(!hidden.contains(&"title".to_string()));
    assert!(ext.hidden_fields("Other").is_empty());
}

#[test]
fn field_fragment_is_built_from_ddic_only() {
    let ext = UiExtension::new(&ddic_with_video(), None);
    let all: Vec<&FormattedField> = ext.formatted.values().flatten().collect();
    let hit = all
        .iter()
        .find(|f| field_fragment_name(&f.field, f.format) == "file_size__bytes");
    assert!(hit.is_some());
    assert!(!is_safe_name("../etc/passwd"));
    assert!(is_safe_name("file_size__bytes"));
}
