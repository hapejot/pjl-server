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
