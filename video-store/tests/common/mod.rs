//! Synthetischer Katalog mit den Formen aus dem Produktivschema.

use std::collections::BTreeMap;

use video_store::introspect::{PgCatalog, PgColumn, PgForeignKey, PgTable};

pub fn col(name: &str, udt: &str, nullable: bool) -> PgColumn {
    PgColumn {
        name: name.into(),
        udt: udt.into(),
        nullable,
        max_length: None,
        precision: None,
        scale: None,
    }
}

pub fn fk(child: &str, col: &str, parent: &str, parent_col: &str, on_delete: char) -> PgForeignKey {
    PgForeignKey {
        child: child.into(),
        child_col: col.into(),
        parent: parent.into(),
        parent_col: parent_col.into(),
        on_delete,
    }
}

pub fn catalog() -> PgCatalog {
    let mut tables: BTreeMap<String, PgTable> = BTreeMap::new();
    let mut add = |name: &str, pk: &[&str], columns: Vec<PgColumn>| {
        tables.insert(
            name.into(),
            PgTable {
                name: name.into(),
                columns,
                pk: pk.iter().map(|s| s.to_string()).collect(),
            },
        );
    };

    add("agents", &["id"], vec![
        col("id", "uuid", false),
        col("name", "varchar", false),
        col("status", "agent_status", false),
    ]);
    add("videos", &["id"], vec![
        col("id", "uuid", false),
        col("agent_id", "uuid", false),
        col("title", "varchar", true),
        col("file_size", "int8", false),
        col("duration_seconds", "float8", true),
        col("scene_id", "text", true),
    ]);
    add("tags", &["id"], vec![col("id", "uuid", false), col("name", "varchar", false)]);
    add("video_tags", &["id"], vec![
        col("id", "uuid", false),
        col("video_id", "uuid", false),
        col("tag_id", "uuid", false),
        col("source", "tag_source", false),
    ]);
    add("video_jobs", &["id"], vec![
        col("id", "uuid", false),
        col("video_id", "uuid", true),
        col("agent_id", "uuid", false),
        col("parameters", "jsonb", true),
    ]);
    add("scenes", &["id"], vec![col("id", "text", false), col("title", "text", true)]);
    add("actors", &["id"], vec![col("id", "text", false), col("name", "text", true)]);
    add("actor_tags", &["id"], vec![col("id", "text", false), col("name", "text", true)]);
    add("scene_paths", &["id"], vec![col("id", "text", false)]);
    add("scene_files", &["id"], vec![
        col("id", "text", false),
        col("path_id", "text", true),
    ]);
    add("actor_scene_map", &[], vec![
        col("actor_id", "text", false),
        col("scene_id", "text", false),
    ]);
    add("actor_tag_map", &[], vec![col("actor_id", "text", false), col("tag_id", "text", false)]);
    add("scene_tag_map", &[], vec![col("scene_id", "text", false), col("tag_id", "text", false)]);
    add("path_scene_map", &["path_id"], vec![
        col("path_id", "text", false),
        col("scene_id", "text", true),
    ]);
    add("training_images", &["id"], vec![
        col("id", "uuid", false),
        col("video_id", "uuid", true),
    ]);
    add("emb_images", &["id"], vec![
        col("id", "uuid", false),
        col("video_id", "uuid", true),
        col("training_image_id", "uuid", true),
    ]);
    add("emb_clip", &["id"], vec![
        col("id", "uuid", false),
        col("image_id", "uuid", false),
        col("batch_id", "uuid", true),
        col("vector", "vector", false),
    ]);
    add("emb_batches", &["id"], vec![col("id", "uuid", false)]);
    add("emb_cluster_runs", &["id"], vec![col("id", "uuid", false)]);
    add("emb_cluster_results", &["run_id", "image_id"], vec![
        col("run_id", "uuid", false),
        col("image_id", "uuid", false),
        col("cluster_id", "int4", false),
    ]);
    add("label_categories", &["key"], vec![col("key", "varchar", false)]);
    add("label_category_options", &["id"], vec![
        col("id", "uuid", false),
        col("category_key", "varchar", false),
        col("value", "varchar", false),
    ]);
    add("faces", &["id"], vec![
        col("id", "int8", false),
        col("name", "varchar", true),
        col("image_data", "bytea", true),
    ]);

    let foreign_keys = vec![
        fk("videos", "agent_id", "agents", "id", 'c'),
        fk("video_tags", "video_id", "videos", "id", 'c'),
        fk("video_tags", "tag_id", "tags", "id", 'c'),
        fk("video_jobs", "agent_id", "agents", "id", 'a'),
        fk("video_jobs", "video_id", "videos", "id", 'c'),
        fk("emb_clip", "image_id", "emb_images", "id", 'c'),
        fk("emb_clip", "batch_id", "emb_batches", "id", 'n'),
        fk("emb_cluster_results", "run_id", "emb_cluster_runs", "id", 'c'),
        fk("emb_cluster_results", "image_id", "emb_images", "id", 'c'),
        fk("label_category_options", "category_key", "label_categories", "key", 'c'),
    ];

    PgCatalog { tables, foreign_keys }
}
