use super::*;
use std::fs;
use std::sync::{Arc, Mutex};
use tracing::{
    field::{Field, Visit},
    Event, Subscriber,
};
use tracing_subscriber::{layer::Context, prelude::*, Layer};

#[derive(Clone)]
struct CapturedLogs(Arc<Mutex<Vec<(tracing::Level, String)>>>);

impl<S: Subscriber> Layer<S> for CapturedLogs {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        struct ScriptMessage(Option<String>);

        impl Visit for ScriptMessage {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                if field.name() == "script_message" {
                    self.0 = Some(format!("{value:?}"));
                }
            }
        }

        let mut message = ScriptMessage(None);
        event.record(&mut message);
        if let Some(message) = message.0 {
            self.0
                .lock()
                .unwrap()
                .push((*event.metadata().level(), message));
        }
    }
}

fn dir_with(files: &[(&str, &str)]) -> tempdir::Dir {
    let d = tempdir::Dir::new();
    for (name, body) in files {
        let path = d.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    d
}

/// Minimales Verzeichnis-Hilfsmittel ohne zusaetzliche Abhaengigkeit.
mod tempdir {
    use std::path::{Path, PathBuf};
    pub struct Dir(PathBuf);
    impl Dir {
        pub fn new() -> Dir {
            let p = std::env::temp_dir().join(format!(
                "script-hooks-{}-{}",
                std::process::id(),
                uuid_like()
            ));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn uuid_like() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            + rand_seed()
    }
    fn rand_seed() -> u128 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::Relaxed) as u128
    }
}

#[test]
fn write_hook_changes_payload() {
    let d = dir_with(&[(
        "Order.rhai",
        r#"
        fn before_create(ctx) {
            ctx.payload.Name = ctx.payload.Name.to_upper();
            ctx.payload.Checked = true;
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut payload = json!({"Name": "abc"});
    hooks.before("Order", "create", &mut payload).unwrap();
    assert_eq!(payload, json!({"Name": "ABC", "Checked": true}));
}

#[test]
fn write_hook_receives_key_and_both_current_variants() {
    let d = dir_with(&[(
        "Order.rhai",
        r#"
        fn before_patch(ctx) {
            if ctx.key.IsActiveEntity && ctx.current.active.Name == "active" &&
                ctx.current.draft.Name == "draft" {
                ctx.payload.Checked = true;
            }
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let key = json!({"ID": "item-1", "IsActiveEntity": true});
    let current = json!({
        "active": {"ID": "item-1", "IsActiveEntity": true, "Name": "active"},
        "draft": {"ID": "item-1", "IsActiveEntity": false, "Name": "draft"}
    });
    let mut payload = json!({"Name": "changed"});
    hooks
        .before_operation("Order", "patch", &mut payload, Some(key), Some(&current))
        .unwrap();
    assert_eq!(payload, json!({"Name": "changed", "Checked": true}));
}

#[test]
fn throw_aborts_with_message() {
    let d = dir_with(&[(
        "Order.rhai",
        r#"
        fn before_create(ctx) {
            if ctx.payload.Qty < 0 { throw "Menge negativ"; }
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut payload = json!({"Qty": -1});
    let err = hooks.before("Order", "create", &mut payload).unwrap_err();
    assert!(err.to_string().contains("Menge negativ"), "{err}");
}

#[test]
fn missing_function_or_set_is_no_hook() {
    let d = dir_with(&[("Order.rhai", "fn after_create(ctx) { ctx }")]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut payload = json!({"a": 1});
    hooks.before("Order", "create", &mut payload).unwrap();
    hooks.before("Other", "create", &mut payload).unwrap();
    assert_eq!(payload, json!({"a": 1}));
}

#[test]
fn list_after_read_can_remove_rows() {
    let d = dir_with(&[(
        "Item.rhai",
        r#"
        fn after_read(ctx) {
            if ctx.kind == "list" {
                ctx.payload = ctx.payload.filter(|r| r.Visible);
            }
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut payload = json!([{"Visible": true}, {"Visible": false}]);
    hooks
        .after_read("Item", "list", None, None, &mut payload)
        .unwrap();
    assert_eq!(payload, json!([{"Visible": true}]));
}

#[test]
fn before_read_extends_filter_as_structure() {
    let d = dir_with(&[(
        "Item.rhai",
        r#"
        fn before_read(ctx) {
            ctx.query.filter = and(ctx.query.filter, eq(prop("Owner"), text("peter")));
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut q = ODataQuery::new();
    hooks.before_read("Item", None, &mut q).unwrap();
    let expected = odata::query::Expr::BinaryOp {
        op: odata::query::BinOp::Eq,
        left: Box::new(odata::query::Expr::Identifier("Owner".into())),
        right: Box::new(odata::query::Expr::String("peter".into())),
    };
    assert_eq!(q.filter, Some(expected));
}

#[test]
fn before_read_reduces_filter_and_keeps_other_side() {
    let d = dir_with(&[(
        "Item.rhai",
        r#"
        fn before_read(ctx) {
            ctx.query.filter = ctx.query.filter.BinaryOp.left;
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut q = ODataQuery::new().filter(odata::query::Expr::BinaryOp {
        op: odata::query::BinOp::And,
        left: Box::new(odata::query::Expr::Identifier("A".into())),
        right: Box::new(odata::query::Expr::Identifier("B".into())),
    });
    hooks.before_read("Item", None, &mut q).unwrap();
    assert_eq!(q.filter, Some(odata::query::Expr::Identifier("A".into())));
}

#[test]
fn invalid_query_returned_is_error() {
    let d = dir_with(&[(
        "Item.rhai",
        r#"fn before_read(ctx) { ctx.query.top = "viel"; ctx }"#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut q = ODataQuery::new();
    assert!(hooks.before_read("Item", None, &mut q).is_err());
}

#[test]
fn function_without_ctx_return_is_error() {
    let d = dir_with(&[("Item.rhai", "fn after_create(ctx) { 1 }")]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut p = json!({});
    let err = hooks.after("Item", "create", &mut p).unwrap_err();
    assert!(err.to_string().contains("zurueckgeben"));
}

#[test]
fn script_log_writes_to_tracing() {
    let d = dir_with(&[(
        "Order.rhai",
        r#"
        fn before_create(ctx) {
            log("created from script");
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let logs = CapturedLogs(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::registry().with(logs.clone());

    tracing::subscriber::with_default(subscriber, || {
        let mut payload = json!({});
        hooks.before("Order", "create", &mut payload).unwrap();
    });

    let logs = logs.0.lock().unwrap();
    assert_eq!(
        *logs,
        vec![(tracing::Level::INFO, "created from script".into())]
    );
}

#[test]
fn script_log_writes_context_and_arrays_as_json_without_changing_payload() {
    let d = dir_with(&[(
        "Order.rhai",
        r#"
        fn before_create(ctx) {
            log(ctx);
            log(ctx.payload.items);
            ctx
        }
        "#,
    )]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let logs = CapturedLogs(Arc::new(Mutex::new(Vec::new())));
    let subscriber = tracing_subscriber::registry().with(logs.clone());
    let original = json!({
        "name": "Order \"demo\"\nsecond line",
        "items": [{"qty": 2, "price": 1.5, "active": true, "note": null}]
    });
    let mut payload = original.clone();

    tracing::subscriber::with_default(subscriber, || {
        hooks.before("Order", "create", &mut payload).unwrap();
    });

    assert_eq!(payload, original);
    let logs = logs.0.lock().unwrap();
    assert_eq!(logs.len(), 2);
    for (level, _) in logs.iter() {
        assert_eq!(*level, tracing::Level::INFO);
    }
    let ctx: Value = serde_json::from_str(&logs[0].1).unwrap();
    assert_eq!(ctx["set"], "Order");
    assert_eq!(ctx["op"], "create");
    assert_eq!(ctx["kind"], "write");
    assert_eq!(ctx["payload"], original);
    let items: Value = serde_json::from_str(&logs[1].1).unwrap();
    assert_eq!(items, original["items"]);
}

#[test]
fn endless_loop_is_stopped() {
    let d = dir_with(&[("Item.rhai", "fn before_create(ctx) { loop { } ctx }")]);
    let hooks = ScriptHooks::load(d.path()).unwrap();
    let mut p = json!({});
    let err = hooks.before("Item", "create", &mut p).unwrap_err();
    assert!(err.to_string().contains("Zeitlimit") || err.to_string().contains("operations"));
}
