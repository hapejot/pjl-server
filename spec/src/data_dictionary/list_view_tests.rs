use super::*;
use crate::ListView;
use crate::package::Package;

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

/// Agent 1:n Video (Video.agent einwertig), Video 1:n Tag (Video._Tags Sammelnavigation).
fn ddic(views: Vec<ListView>) -> DataDictionary {
    let mut ddic = DataDictionary::empty();
    ddic.add_package(Package::new("core"));
    ddic.add_entity(EntitySpec::new("Agent", "core", "Agents").field(atom("name")));
    let mut video = EntitySpec::new("Video", "core", "Videos")
        .field(atom("name"))
        .field(atom("file_size"));
    for v in views {
        video = video.list_view(v);
    }
    ddic.add_entity(video);
    ddic.add_entity(EntitySpec::new("Tag", "core", "Tags").field(atom("name")));
    ddic.add_relationship(
        Relationship::new("videos_agent_id", "core")
            .with_one("Agent", "_Videos")
            .with_many("Video", "agent"),
    );
    ddic.add_relationship(
        Relationship::new("tags_video_id", "core")
            .with_one("Video", "_Tags")
            .with_many("Tag", "video"),
    );
    ddic
}

#[test]
fn valid_views_pass() {
    let d = ddic(vec![
        ListView::new("default", &["name", "agent", "file_size"]),
        ListView::new("review", &["file_size"]),
    ]);
    assert!(d.validate_list_views().is_ok());
}

#[test]
fn unknown_column_is_rejected() {
    let d = ddic(vec![ListView::new("default", &["nope"])]);
    let err = d.validate_list_views().unwrap_err();
    assert!(err.contains("'nope' existiert nicht"), "{err}");
}

#[test]
fn to_many_navigation_is_rejected_as_column() {
    let d = ddic(vec![ListView::new("default", &["_Tags"])]);
    let err = d.validate_list_views().unwrap_err();
    assert!(err.contains("Sammelnavigation"), "{err}");
}

#[test]
fn duplicate_view_name_and_column_are_rejected() {
    let d = ddic(vec![
        ListView::new("default", &["name"]),
        ListView::new("default", &["name"]),
    ]);
    assert!(d.validate_list_views().unwrap_err().contains("doppelt"));

    let d = ddic(vec![ListView::new("x", &["name", "name"])]);
    assert!(d.validate_list_views().unwrap_err().contains("doppelt"));
}

#[test]
fn column_width_is_validated() {
    use crate::{ListColumn, Priority};
    for ok in ["120px", "15rem", "2.5em", "20%", "auto"] {
        let d = ddic(vec![ListView::with_columns(
            "default",
            vec![
                ListColumn::new("name")
                    .with_width(ok)
                    .with_priority(Priority::Low),
            ],
        )]);
        assert!(d.validate_list_views().is_ok(), "{ok}");
    }
    for bad in ["", "px", "12", "12pt", "abc", "1.2.3px", "-5px", "10 px"] {
        let d = ddic(vec![ListView::with_columns(
            "default",
            vec![ListColumn::new("name").with_width(bad)],
        )]);
        let err = d.validate_list_views().unwrap_err();
        assert!(err.contains("Breite"), "{bad}: {err}");
    }
}

#[test]
fn invalid_name_and_empty_columns_are_rejected() {
    let d = ddic(vec![ListView::new("a#b", &["name"])]);
    assert!(
        d.validate_list_views()
            .unwrap_err()
            .contains("Ansichtsname")
    );

    let d = ddic(vec![ListView::new("x", &[])]);
    assert!(
        d.validate_list_views()
            .unwrap_err()
            .contains("keine Spalten")
    );
}
