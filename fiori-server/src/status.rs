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
use serde_json::{Map, Value, json};
use tracing::field::{Field, Visit};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::AppState;
use crate::config::ServerConfig;
use crate::extension::Extension;

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
mod tests;
