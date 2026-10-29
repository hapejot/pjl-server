//! Laufzeitstatus des Servers: Zaehler, Log-Ringbuffer und Konfiguration.
//!
//! Der [`StatusHub`] ist prozessweit (siehe [`hub`]). Dadurch koennen Tracing-
//! Layer, Request-Zaehler, HTTP-Route und spaeter die 3270-Schnittstelle
//! dieselbe Instanz nutzen, ohne dass `AppState` umgebaut werden muss.
//!
//! Erweiterungen liefern eigene Abschnitte ueber [`crate::extension::Extension::status`].

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use axum::extract::{Query, Request, State};
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::config::ServerConfig;
use crate::extension::Extension;
use crate::AppState;

/// Anzahl der Log-Eintraege im Ringbuffer.
pub const LOG_CAPACITY: usize = 500;

/// Ein Log-Eintrag aus dem Ringbuffer.
#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub ts_ms: u64,
    pub level: String,
    pub target: String,
    pub message: String,
}

/// Prozessweiter Laufzeitstatus.
pub struct StatusHub {
    started_unix_ms: u64,
    started: Instant,
    requests: AtomicU64,
    errors: AtomicU64,
    logs: Mutex<VecDeque<LogEntry>>,
    config: RwLock<Value>,
}

static HUB: OnceLock<Arc<StatusHub>> = OnceLock::new();

/// Die prozessweite Instanz. Beim ersten Aufruf wird sie angelegt.
pub fn hub() -> Arc<StatusHub> {
    HUB.get_or_init(|| Arc::new(StatusHub::new())).clone()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl StatusHub {
    fn new() -> Self {
        Self {
            started_unix_ms: now_ms(),
            started: Instant::now(),
            requests: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            logs: Mutex::new(VecDeque::with_capacity(LOG_CAPACITY)),
            config: RwLock::new(Value::Null),
        }
    }

    /// Zaehlt eine beantwortete Anfrage. Status >= 500 gilt als Fehler.
    pub fn record_request(&self, status: u16) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if status >= 500 {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Haengt einen Log-Eintrag an; der aelteste fliegt bei voller Kapazitaet raus.
    pub fn push_log(&self, entry: LogEntry) {
        let mut logs = self.logs.lock().unwrap_or_else(|p| p.into_inner());
        if logs.len() == LOG_CAPACITY {
            logs.pop_front();
        }
        logs.push_back(entry);
    }

    /// Die letzten `limit` Log-Eintraege, aelteste zuerst.
    pub fn logs(&self, limit: usize) -> Vec<LogEntry> {
        let logs = self.logs.lock().unwrap_or_else(|p| p.into_inner());
        let skip = logs.len().saturating_sub(limit);
        logs.iter().skip(skip).cloned().collect()
    }

    /// Fenster aus dem Ringbuffer: `count` Eintraege, `offset` Eintraege vom
    /// neuesten Ende aus gezaehlt (0 = die neuesten). Aelteste zuerst.
    pub fn logs_window(&self, offset: usize, count: usize) -> Vec<LogEntry> {
        let logs = self.logs.lock().unwrap_or_else(|p| p.into_inner());
        let end = logs.len().saturating_sub(offset);
        let start = end.saturating_sub(count);
        logs.iter().skip(start).take(end - start).cloned().collect()
    }

    /// Anzahl der Eintraege im Ringbuffer.
    pub fn log_count(&self) -> usize {
        self.logs.lock().unwrap_or_else(|p| p.into_inner()).len()
    }

    /// Setzt die (bereits bereinigte) Konfiguration fuer den Snapshot.
    pub fn set_config(&self, value: Value) {
        *self.config.write().unwrap_or_else(|p| p.into_inner()) = value;
    }

    /// Gesamtbild fuer `/status`. Erweiterungen liefern ihre Abschnitte selbst.
    pub fn snapshot(&self, extensions: &[Arc<dyn Extension>]) -> Value {
        let mut ext = Map::new();
        for e in extensions {
            if let Some((name, value)) = e.status() {
                ext.insert(name, value);
            }
        }
        let log_count = self.logs.lock().unwrap_or_else(|p| p.into_inner()).len();
        json!({
            "server": {
                "pid": std::process::id(),
                "started_at_ms": self.started_unix_ms,
                "uptime_s": self.started.elapsed().as_secs(),
            },
            "requests": {
                "total": self.requests.load(Ordering::Relaxed),
                "errors_5xx": self.errors.load(Ordering::Relaxed),
            },
            "logs": { "capacity": LOG_CAPACITY, "count": log_count },
            "config": self.config.read().unwrap_or_else(|p| p.into_inner()).clone(),
            "extensions": Value::Object(ext),
        })
    }
}

/// Konfiguration ohne Geheimnisse (z. B. keine Postgres-URL mit Passwort).
pub fn config_json(config: &ServerConfig) -> Value {
    json!({
        "mode": config.mode.label(),
        "port": config.port,
        "bind": config.bind.to_string(),
        "data_dir": config.data_dir.display().to_string(),
        "web_root": config.web_root.display().to_string(),
        "ddic": config.ddic.as_ref().map(|p| p.display().to_string()),
        "ddic_overlay": config.ddic_overlay.as_ref().map(|p| p.display().to_string()),
        "schema": config.schema.as_ref().map(|p| p.display().to_string()),
        "export_dir": config.export_dir.display().to_string(),
        "auth_user_header": config.auth_user_header,
        "auth_required": config.auth_required,
        "store": if config.store_pg_url.is_some() { "postgres" } else { "sqlite" },
        "term3270": config.term3270_port.map(|p| format!("{}:{p}", config.term3270_bind)),
    })
}

// ---------------------------------------------------------------------
// Tracing-Layer: schreibt jedes Ereignis in den Ringbuffer
// ---------------------------------------------------------------------

/// Layer fuer `tracing_subscriber`. In den Binaries mit `.with(log_layer())` einhaengen.
#[derive(Debug, Default, Clone, Copy)]
pub struct RingLayer;

/// Liefert den Layer fuer die Subscriber-Kette.
pub fn log_layer() -> RingLayer {
    RingLayer
}

struct MessageVisitor(String);

impl Visit for MessageVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.0 = format!("{value:?}");
        } else {
            if !self.0.is_empty() {
                self.0.push(' ');
            }
            let _ = write!(self.0, "{}={value:?}", field.name());
        }
    }
}

impl<S> Layer<S> for RingLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = MessageVisitor(String::new());
        event.record(&mut visitor);
        let meta = event.metadata();
        hub().push_log(LogEntry {
            ts_ms: now_ms(),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            message: visitor.0,
        });
    }
}

// ---------------------------------------------------------------------
// HTTP: Zaehler-Middleware und Routen
// ---------------------------------------------------------------------

/// Middleware: zaehlt jede Anfrage mit ihrem Antwortstatus.
pub async fn count_requests(req: Request, next: Next) -> Response {
    let res = next.run(req).await;
    hub().record_request(res.status().as_u16());
    res
}

#[derive(Debug, Deserialize)]
pub struct LogQuery {
    limit: Option<usize>,
}

async fn status_handler(State(state): State<AppState>) -> Json<Value> {
    Json(hub().snapshot(&state.extensions))
}

async fn logs_handler(Query(q): Query<LogQuery>) -> Json<Vec<LogEntry>> {
    Json(hub().logs(q.limit.unwrap_or(100).min(LOG_CAPACITY)))
}

/// Routen `/status` (Snapshot) und `/status/logs?limit=N` (Ringbuffer).
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/status", get(status_handler))
        .route("/status/logs", get(logs_handler))
}

#[cfg(test)]
mod tests {
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
        assert_eq!(all.last().unwrap().message, format!("eintrag {}", LOG_CAPACITY + 9));
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
        assert_eq!(msgs(hub.logs_window(0, 3)), vec!["eintrag 7", "eintrag 8", "eintrag 9"]);
        assert_eq!(msgs(hub.logs_window(3, 3)), vec!["eintrag 4", "eintrag 5", "eintrag 6"]);
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
}
