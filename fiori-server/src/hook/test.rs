use std::sync::Mutex;
use std::sync::RwLock;

use odata_yaml_store::YamlStore;
use serde_json::json;
use tracing::Level;

use super::*;
/// Ein Beispiel-Sink, der das Skelett füllbar lässt: jeder Hook speichert Zahlen, die du testweise
/// abrufen und für Prüfungen nutzen kannst. Dein eigener Hook kopiert hier diese Felder.
#[derive(Default)]
struct HookData {
    pub created: u64,
    pub activated: u64,
}
#[derive(Default)]
pub struct ExampleHook {
    inner: RwLock<HookData>,
}

impl ExampleHook {
    pub fn create_count(&self) -> u64 {
        self.inner.try_read().unwrap().created
    }
}

#[async_trait::async_trait]
impl Hook for ExampleHook {
    async fn after(
        &self,
        store: &Extender,
        set_name: &str,
        op: Operation,
        value: &mut DataRecord,
    ) -> Result<(), HookError> {
        debug!(?op, ?value);

        // Track whether we need to create an OrderItem
        let should_create_item = {
            let mut w = self
                .inner
                .try_write()
                .map_err(|_| HookError::Hook("Failed to acquire write lock".to_string()))?;

            match set_name {
                "OrderSet" => match op {
                    Operation::Create => {
                        w.created += 1;
                        true
                    }
                    Operation::DraftActivate => {
                        w.activated += 1;
                        false
                    }
                    _ => false,
                },
                _ => false,
            }
        }; // Lock is released here

        // Create OrderItem outside the lock
        if should_create_item {
            store
                .create_entity(
                    "OrderItemSet",
                    &json!({"Pos": "10", "Order_ID": value.id()}),
                )
                .await?;
        }

        Ok(())
    }
}

#[tokio::test]
async fn example_hook() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    let ystore = Arc::new(YamlStore::open("testdata")?);
    let hook = Arc::new(ExampleHook::default());
    let store = Extender::new(ystore, hook.clone())?;
    store
        .create_entity("OrderSet", &json!({"SoldTo": "Peter"}))
        .await?;
    assert_eq!(hook.create_count(), 1);
    Ok(())
}

#[derive(Default)]
struct CurrentRecordsHook {
    seen: Mutex<Vec<(Operation, bool, bool)>>,
}

#[async_trait::async_trait]
impl Hook for CurrentRecordsHook {
    async fn before_with_current(
        &self,
        _store: &Extender,
        _set_name: &str,
        op: Operation,
        _key: Option<&EntityKey>,
        _value: &mut Value,
        current: &CurrentRecords,
    ) -> Result<(), HookError> {
        self.seen
            .lock()
            .map_err(|e| HookError::Hook(e.to_string()))?
            .push((op, current.active.is_some(), current.draft.is_some()));
        Ok(())
    }
}

#[tokio::test]
async fn every_write_hook_receives_active_and_draft_records() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let inner = Arc::new(YamlStore::open(dir.path())?);
    let hook = Arc::new(CurrentRecordsHook::default());
    let store = Extender::new(inner, hook.clone())?;
    let id = uuid::Uuid::new_v4();
    let active_key = EntityKey::new(id, true);
    let draft_key = active_key.sibling();

    store
        .create_entity(
            "ItemSet",
            &json!({ "ID": id.to_string(), "IsActiveEntity": true, "Name": "active" }),
        )
        .await?;
    store.draft_edit("ItemSet", &active_key).await?;
    store
        .patch_entity("ItemSet", &draft_key, &json!({ "Name": "draft" }))
        .await?;
    store.draft_prepare("ItemSet", &draft_key).await?;
    store.draft_activate("ItemSet", &draft_key).await?;
    let results = store
        .write_entities(
            "ItemSet",
            vec![WriteOp::Patch(
                active_key.clone(),
                json!({ "Name": "batch" }),
            )],
        )
        .await;
    assert!(matches!(results.as_slice(), [Ok(Some(_))]));
    store.delete_entity("ItemSet", &active_key).await?;

    let seen = hook.seen.lock().unwrap();
    assert_eq!(
        *seen,
        vec![
            (Operation::Create, false, false),
            (Operation::DraftEdit, true, false),
            (Operation::Patch, true, true),
            (Operation::DraftPrepare, true, true),
            (Operation::DraftActivate, true, true),
            (Operation::Patch, true, false),
            (Operation::Delete, true, false),
        ]
    );
    Ok(())
}
