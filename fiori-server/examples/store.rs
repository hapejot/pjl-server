use odata::query::ODataQuery;
use odata_yaml_store::{DataStore, DraftDataStore, EntityKey, YamlStore};
use serde_json::json;
use tracing::{Level, info};
use tracing_subscriber::{fmt, prelude::*};

fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(fmt::layer()) // hübsche Konsolenausgabe
        // .with(EnvFilter::from_default_env()) // Steuerung über RUST_LOG
        .with(tracing_subscriber::filter::LevelFilter::from_level(
            Level::DEBUG,
        ))
        .init();
    let relationset = "RelationSet";
    let store = YamlStore::open("/home/peter/pjl-server/data")?;
    let mut query = ODataQuery::default();
    query.select = vec!["id".into()];
    let result = store.get_collection(relationset, &query)?;
    if let Some(f) = result.iter().find(|x| !x.is_active_entity()) {
        let key = EntityKey::from(f);
        let patch = json!({"change_count": f["change_count"].as_u64().unwrap_or(0) + 1});
        store.patch_entity(relationset, &key, &patch)?;
        store.draft_activate(relationset, &key)?;
    } else {
        if let Some(f) = result.iter().find(|x| x.is_active_entity()) {
            let key = EntityKey::from(f);
            info!("edit record: {:?}", key);
            store.draft_edit(relationset, &key)?;
        }
    }
    Ok(())
}
