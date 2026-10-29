//! Laufzeitstatus der Video-Dienste fuer den fiori-server (`/status`, 3270).
//!
//! Gezaehlt wird, was der Server selbst sieht: Ergebnisse der Proxy-Anfragen
//! an Thumbnail- und Stream-Upstream sowie Treffer und Neuaufbau des
//! Tag-Ranking-Caches. Der Thumbnail-Proxy hat keinen eigenen Cache; die
//! Bilder werden nur mit `Cache-Control: max-age` an den Browser gegeben.
//!
//! Die Erreichbarkeit des Upstreams wird nicht aktiv geprueft (das waere ein
//! blockierender Aufruf im Statuspfad). Angezeigt wird das Ergebnis der letzten
//! Anfrage und der letzte Fehler.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Zaehler eines Proxy-Dienstes.
pub struct Service {
    ok: AtomicU64,
    err: AtomicU64,
    last_ok_ms: AtomicU64,
    last_err: Mutex<Option<(u64, String)>>,
}

impl Service {
    const fn new() -> Self {
        Self {
            ok: AtomicU64::new(0),
            err: AtomicU64::new(0),
            last_ok_ms: AtomicU64::new(0),
            last_err: Mutex::new(None),
        }
    }

    /// Bewertet eine Antwort. Status < 400 gilt als Erfolg; 404 vom Upstream
    /// (Datei fehlt) zaehlt ebenfalls als Fehler, weil das Bild nicht kommt.
    pub fn record(&self, status: u16) {
        if status < 400 {
            self.ok.fetch_add(1, Ordering::Relaxed);
            self.last_ok_ms.store(now_ms(), Ordering::Relaxed);
        } else {
            self.err.fetch_add(1, Ordering::Relaxed);
            let msg = format!("HTTP {status}");
            *self.last_err.lock().unwrap_or_else(|p| p.into_inner()) = Some((now_ms(), msg));
        }
    }

    fn json(&self, upstream: &str) -> Value {
        let last_err = self.last_err.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let last_ok = self.last_ok_ms.load(Ordering::Relaxed);
        json!({
            "upstream": upstream,
            "ok": self.ok.load(Ordering::Relaxed),
            "errors": self.err.load(Ordering::Relaxed),
            "last_ok_ms": if last_ok == 0 { Value::Null } else { json!(last_ok) },
            "last_error_ms": last_err.as_ref().map(|(t, _)| json!(t)).unwrap_or(Value::Null),
            "last_error": last_err.map(|(_, m)| m).unwrap_or_default(),
        })
    }
}

static THUMBNAILS: Service = Service::new();
static STREAM: Service = Service::new();
static TAG_HITS: AtomicU64 = AtomicU64::new(0);
static TAG_REFRESHES: AtomicU64 = AtomicU64::new(0);
static TAG_LAST_REFRESH_MS: AtomicU64 = AtomicU64::new(0);

pub fn thumbnail_result(status: u16) {
    THUMBNAILS.record(status);
}

pub fn stream_result(status: u16) {
    STREAM.record(status);
}

pub fn tag_cache_hit() {
    TAG_HITS.fetch_add(1, Ordering::Relaxed);
}

pub fn tag_cache_refreshed() {
    TAG_REFRESHES.fetch_add(1, Ordering::Relaxed);
    TAG_LAST_REFRESH_MS.store(now_ms(), Ordering::Relaxed);
}

/// Abschnitt Thumbnails. Der Proxy hat keinen eigenen Cache, daher steht das im Status.
pub fn thumbnails_json(upstream: &str) -> Value {
    let mut v = THUMBNAILS.json(upstream);
    v["cache"] = json!("keiner (Proxy; Browser-Cache max-age=86400)");
    v
}

/// Abschnitt Video-Stream.
pub fn stream_json(upstream: &str) -> Value {
    STREAM.json(upstream)
}

/// Abschnitt Tag-Facetten mit dem 60-s-Ranking-Cache.
pub fn tags_json() -> Value {
    let last = TAG_LAST_REFRESH_MS.load(Ordering::Relaxed);
    let age = if last == 0 {
        Value::Null
    } else {
        json!(now_ms().saturating_sub(last) / 1000)
    };
    json!({
        "cache": "Ranking, TTL 60 s",
        "hits": TAG_HITS.load(Ordering::Relaxed),
        "refreshes": TAG_REFRESHES.load(Ordering::Relaxed),
        "age_s": age,
    })
}

/// Gesamtsicht fuer die 3270-Seite.
pub fn overview_json(thumb_upstream: &str, stream_upstream: &str) -> Value {
    json!({
        "thumbnails": thumbnails_json(thumb_upstream),
        "stream": stream_json(stream_upstream),
        "tags": tags_json(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_counts_and_remembers_last_error() {
        let s = Service::new();
        s.record(200);
        s.record(502);
        let v = s.json("http://x");
        assert_eq!(v["ok"], 1);
        assert_eq!(v["errors"], 1);
        assert_eq!(v["last_error"], "HTTP 502");
        assert!(v["last_ok_ms"].is_number());
    }

    #[test]
    fn fresh_service_reports_no_times() {
        let v = Service::new().json("u");
        assert!(v["last_ok_ms"].is_null());
        assert!(v["last_error_ms"].is_null());
    }
}
