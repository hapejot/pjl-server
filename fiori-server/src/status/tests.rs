use super::*;

fn entry(i: usize) -> LogEntry {
    LogEntry {
        ts_ms: i as u64,
        level: "INFO".into(),
        target: "test".into(),
        message: format!("eintrag {i}"),
    }
}

#[test]
fn ringbuffer_keeps_only_last_capacity_entries() {
    let hub = StatusHub::new();
    for i in 0..LOG_CAPACITY + 10 {
        hub.push_log(entry(i));
    }
    let all = hub.logs(usize::MAX);
    assert_eq!(all.len(), LOG_CAPACITY);
    assert_eq!(all.first().unwrap().message, "eintrag 10");
    assert_eq!(
        all.last().unwrap().message,
        format!("eintrag {}", LOG_CAPACITY + 9)
    );
}

#[test]
fn logs_limit_returns_newest_in_order() {
    let hub = StatusHub::new();
    for i in 0..5 {
        hub.push_log(entry(i));
    }
    let last2: Vec<_> = hub.logs(2).into_iter().map(|e| e.message).collect();
    assert_eq!(last2, vec!["eintrag 3", "eintrag 4"]);
}

#[test]
fn window_pages_backwards_from_newest() {
    let hub = StatusHub::new();
    for i in 0..10 {
        hub.push_log(entry(i));
    }
    let msgs = |v: Vec<LogEntry>| v.into_iter().map(|e| e.message).collect::<Vec<_>>();
    assert_eq!(
        msgs(hub.logs_window(0, 3)),
        vec!["eintrag 7", "eintrag 8", "eintrag 9"]
    );
    assert_eq!(
        msgs(hub.logs_window(3, 3)),
        vec!["eintrag 4", "eintrag 5", "eintrag 6"]
    );
    assert_eq!(msgs(hub.logs_window(9, 3)), vec!["eintrag 0"]);
    assert!(hub.logs_window(20, 3).is_empty());
}

#[test]
fn counters_count_requests_and_5xx_errors() {
    let hub = StatusHub::new();
    hub.record_request(200);
    hub.record_request(404);
    hub.record_request(503);
    let snap = hub.snapshot(&[]);
    assert_eq!(snap["requests"]["total"], 3);
    assert_eq!(snap["requests"]["errors_5xx"], 1);
}

struct Demo;
impl Extension for Demo {
    fn status(&self) -> Option<(String, Value)> {
        Some(("demo".into(), json!({ "treffer": 7 })))
    }
}

#[test]
fn snapshot_has_sections_and_extension_data() {
    let hub = StatusHub::new();
    hub.set_config(json!({ "mode": "runtime" }));
    let exts: Vec<Arc<dyn Extension>> = vec![Arc::new(Demo)];
    let snap = hub.snapshot(&exts);
    assert!(snap["server"]["pid"].is_number());
    assert_eq!(snap["config"]["mode"], "runtime");
    assert_eq!(snap["extensions"]["demo"]["treffer"], 7);
    assert_eq!(snap["logs"]["capacity"], LOG_CAPACITY);
}

#[test]
fn config_json_hides_postgres_url() {
    let mut cfg = ServerConfig::defaults(crate::config::ServerMode::Runtime);
    cfg.store_pg_url = Some("postgres://user:geheim@host/db".into());
    let v = config_json(&cfg);
    assert_eq!(v["store"], "postgres");
    assert!(!v.to_string().contains("geheim"));
}
