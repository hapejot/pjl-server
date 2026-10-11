mod common;

use data_store::{Cardinality, Ownership};
use video_store::map::{self, KeyMode};

fn model() -> map::Model {
    map::build_model(&common::catalog()).expect("model builds")
}

fn assoc<'a>(schema: &'a data_store::Schema, set: &str, name: &str) -> &'a data_store::Association {
    schema
        .set(set)
        .unwrap()
        .associations()
        .find(|a| a.name == name)
        .unwrap_or_else(|| panic!("{set}.{name} fehlt"))
}

#[test]
fn naming_rules() {
    assert_eq!(map::entity_name("videos"), "Video");
    assert_eq!(map::entity_name("label_categories"), "LabelCategory");
    assert_eq!(map::entity_name("faces"), "Face");
    assert_eq!(map::entity_name("emb_dinov2_thing"), "EmbDinov2Thing");
    assert_eq!(map::entity_name("emb_dinov2"), "EmbDinov2");
    assert_eq!(map::nav_name("video_id"), "video");
    assert_eq!(map::nav_name("category_key"), "category");
    assert_eq!(map::collection_nav("video_tags"), "_VideoTags");
}

#[test]
fn schema_derives_and_every_set_is_draft_enabled() {
    let schema = model().schema().unwrap();
    for name in ["VideoSet", "VideoTagSet", "SceneSet", "EmbClipSet"] {
        assert!(schema.contains(name), "{name} fehlt");
    }
    for set in schema.sets() {
        let keys: Vec<_> = set.key_columns().iter().map(|c| c.name.clone()).collect();
        assert_eq!(keys, vec!["ID", "IsActiveEntity"], "{}", set.name);
    }
}

#[test]
fn every_entity_has_name_and_no_renamed_source_remains() {
    let schema = model().schema().unwrap();
    for set in schema.sets() {
        assert!(set.has_column("name"), "{} ohne name", set.name);
    }
    assert!(!schema.set("VideoSet").unwrap().has_column("file_name"));
    assert!(!schema.set("SceneSet").unwrap().has_column("title"));
}

#[test]
fn cascade_on_not_null_becomes_composition_once_per_child() {
    let schema = model().schema().unwrap();
    // video_tags: video_id und tag_id cascaden; genau einer wird Komposition.
    assert_eq!(assoc(&schema, "VideoTagSet", "video").ownership, Ownership::Composition);
    assert_eq!(assoc(&schema, "VideoTagSet", "tag").ownership, Ownership::Association);
    // Parent-Seite: Video hat _VideoTags als Komposition, ToMany.
    let tags = assoc(&schema, "VideoSet", "_VideoTags");
    assert_eq!(tags.cardinality, Cardinality::ToMany);
    assert_eq!(tags.ownership, Ownership::Composition);
    // videos.agent_id cascadet -> Komposition des Agents
    assert_eq!(assoc(&schema, "VideoSet", "agent").ownership, Ownership::Composition);
}

#[test]
fn override_forces_video_jobs_under_agent() {
    let schema = model().schema().unwrap();
    assert_eq!(assoc(&schema, "VideoJobSet", "agent").ownership, Ownership::Composition);
    assert_eq!(assoc(&schema, "VideoJobSet", "video").ownership, Ownership::Association);
}

#[test]
fn set_null_and_implicit_fks_are_associations() {
    let schema = model().schema().unwrap();
    assert_eq!(assoc(&schema, "EmbClipSet", "batch").ownership, Ownership::Association);
    assert_eq!(assoc(&schema, "VideoSet", "scene").ownership, Ownership::Association);
    assert!(schema.set("VideoSet").unwrap().has_column("scene_ID"));
}

#[test]
fn fk_column_replaces_scalar_column() {
    let schema = model().schema().unwrap();
    let set = schema.set("VideoTagSet").unwrap();
    assert!(set.has_column("video_ID"));
    assert!(!set.has_column("video_id"));
}

#[test]
fn key_modes() {
    let m = model();
    assert!(matches!(m.tables["videos"].key, KeyMode::Uuid { .. }));
    assert!(matches!(m.tables["scenes"].key, KeyMode::Legacy { .. }));
    assert!(matches!(m.tables["faces"].key, KeyMode::Legacy { .. }));
    assert!(matches!(m.tables["actor_scene_map"].key, KeyMode::Synth { ref columns } if columns.is_empty()));
    assert!(matches!(m.tables["emb_cluster_results"].key, KeyMode::Synth { ref columns } if columns.len() == 2));
    let schema = m.schema().unwrap();
    assert!(schema.set("SceneSet").unwrap().has_column("legacy_id"));
    assert!(!schema.set("VideoSet").unwrap().has_column("legacy_id"));
}

#[test]
fn edm_mapping() {
    let schema = model().schema().unwrap();
    let video = schema.set("VideoSet").unwrap();
    assert!(matches!(
        video.get_column("duration_seconds").unwrap().data_type,
        data_store::DataType::Decimal { .. }
    ));
    assert!(matches!(
        video.get_column("file_size").unwrap().data_type,
        data_store::DataType::Int64
    ));
    let faces = schema.set("FaceSet").unwrap();
    assert!(matches!(
        faces.get_column("image_data").unwrap().data_type,
        data_store::DataType::Binary
    ));
    let clip = schema.set("EmbClipSet").unwrap();
    assert!(matches!(
        clip.get_column("vector").unwrap().data_type,
        data_store::DataType::String { .. }
    ));
}

#[test]
fn ddic_round_trips_through_json() {
    let m = model();
    let json = serde_json::to_string(&m.ddic).unwrap();
    let back: spec::data_dictionary::DataDictionary = serde_json::from_str(&json).unwrap();
    assert_eq!(back.entities().len(), m.ddic.entities().len());
    assert_eq!(back.relationships().count(), m.ddic.relationships().count());
    spec_schema::schema_from_ddic(&back).unwrap();
}

#[test]
fn list_views_from_constant_reach_the_video_entity() {
    let model = model();
    let video = model.ddic.entities().into_iter().find(|e| e.name() == "Video").unwrap();
    let views = video.list_views();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].name, "default");
    assert_eq!(views[0].columns, vec!["name", "file_size", "scene", "agent"]);
    // Die Konstante ist gegen das aktivierte Modell validiert.
    model.ddic.validate_list_views().expect("LIST_VIEWS valide");
}
