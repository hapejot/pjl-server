//! 3270-Schnittstelle fuer Statusansichten (Bibliothek `srv-3270`).
//!
//! Aktiv nur, wenn `TERM3270_PORT` gesetzt ist. Die Startseite (`home`) zeigt
//! den Zustand; in der Befehlszeile wechselt man mit `logs`, `ext`, `config`
//! oder einem Namen aus einer Erweiterung. PF3 geht zurueck, PF12 beendet.
//! Im Log blaettern PF7 (aelter) und PF8 (neuer), Enter aktualisiert.
//!
//! Alle Statusansichten lesen nur aus dem [`StatusHub`]. Ausnahme sind die
//! Befehlszeile: Navigation zwischen den Statusansichten.

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use srv_3270::{Action, Aid, App, Color, FieldAttribute, FieldId, Screen, Server, View};
use tracing::{error, info};

use crate::config::ServerConfig;
use crate::extension::Extension;
use crate::status::{hub, StatusHub};
use crate::AppState;

/// Geteilter Zustand jeder 3270-Sitzung.
pub type Hub = Arc<StatusHub>;

/// Fabrik fuer eine Erweiterungs-Ansicht.
pub type ViewFactory = Box<dyn Fn() -> Box<dyn View<Hub>> + Send + Sync>;

type SharedFactory = Arc<dyn Fn() -> Box<dyn View<Hub>> + Send + Sync>;

/// Spalte, ab der Werte stehen.
const VAL_COL: u16 = 24;

/// Newtype: erlaubt, beliebige `Box<dyn View<Hub>>` (auch aus Erweiterungen)
/// bei `Server::view` zu registrieren.
pub struct DynView(pub Box<dyn View<Hub>>);

#[async_trait]
impl View<Hub> for DynView {
    async fn enter(&mut self, app: &mut App<Hub>, params: Value) -> Result<(), String> {
        self.0.enter(app, params).await
    }

    fn render(&self, app: &App<Hub>) -> Screen {
        self.0.render(app)
    }

    async fn handle(
        &mut self,
        app: &mut App<Hub>,
        aid: Aid,
        values: &[(FieldId, String)],
    ) -> Action {
        self.0.handle(app, aid, values).await
    }
}

/// Inhalt einer Seite. Parameter: Hub, Snapshot, Zeilenzahl, Blaetter-Offset, Seitennamen.
type Content = fn(&Hub, &Value, usize, usize, &[&'static str]) -> Vec<(String, String)>;

/// Eine Statusseite. `scroll` ist nur fuer Seiten mit `paged` wirksam.
struct Page {
    title: String,
    content: Content,
    names: Arc<Vec<&'static str>>,
    exts: Arc<Vec<Arc<dyn Extension>>>,
    paged: bool,
    scroll: usize,
}

/// Zeilen, die eine Seite bei `rows` Bildschirmzeilen anzeigt.
fn page_lines(rows: u16) -> usize {
    (rows as usize).saturating_sub(6).max(1)
}

#[async_trait]
impl View<Hub> for Page {
    fn render(&self, app: &App<Hub>) -> Screen {
        let (rows, cols) = (app.rows, app.cols);
        let mut s = Screen::new(rows, cols);
        let max = page_lines(rows);

        let snap = app.shared.snapshot(&self.exts);
        let lines = (self.content)(&app.shared, &snap, max, self.scroll, &self.names);

        let title = if self.paged {
            let total = app.shared.log_count();
            let end = total.saturating_sub(self.scroll);
            let start = end.saturating_sub(max);
            if total == 0 {
                format!("{} (leer)", self.title)
            } else {
                format!("{}  Zeilen {}-{} von {}", self.title, start + 1, end, total)
            }
        } else {
            self.title.clone()
        };
        s.text(0, 0, &title, FieldAttribute::protected_intensified(), Some(Color::Blue));

        for (i, (label, value)) in lines.iter().take(max).enumerate() {
            let row = 2 + i as u16;
            if label.is_empty() {
                s.text(row, 0, value, FieldAttribute::normal(), Some(Color::Green));
            } else {
                s.text(row, 0, label, FieldAttribute::normal(), Some(Color::Blue));
                s.text(row, VAL_COL, value, FieldAttribute::normal(), Some(Color::Green));
            }
        }

        let cmd_row = rows.saturating_sub(3);
        s.text(cmd_row, 0, "Befehl ==>", FieldAttribute::normal(), Some(Color::Blue));
        s.input(FieldId::Command, cmd_row, 11, cols.saturating_sub(12), "");
        s.focus(FieldId::Command);

        if !app.msg.is_empty() {
            s.text(
                rows.saturating_sub(2),
                0,
                &app.msg,
                FieldAttribute::protected_intensified(),
                Some(Color::Yellow),
            );
        }
        let help = if self.paged {
            "PF3=Zurueck PF7=Aelter PF8=Neuer PF12=Ende Enter=Aktualisieren"
        } else {
            "PF3=Zurueck PF12=Ende Enter=Aktualisieren  Befehl: Seitenname"
        };
        s.text(rows.saturating_sub(1), 0, help, FieldAttribute::normal(), Some(Color::Blue));

        s
    }

    async fn handle(
        &mut self,
        app: &mut App<Hub>,
        aid: Aid,
        values: &[(FieldId, String)],
    ) -> Action {
        app.msg.clear();
        let step = page_lines(app.rows);
        match aid {
            Aid::Pf(3) => Action::Back,
            Aid::Pf(12) => Action::Exit,
            Aid::Pf(7) if self.paged => {
                let max_scroll = app.shared.log_count().saturating_sub(step);
                self.scroll = (self.scroll + step).min(max_scroll);
                Action::Stay
            }
            Aid::Pf(8) if self.paged => {
                self.scroll = self.scroll.saturating_sub(step);
                Action::Stay
            }
            Aid::Enter => {
                let raw = command_text(values);
                if raw.is_empty() {
                    return Action::Stay;
                }
                let cmd = raw.to_lowercase();
                match self.names.iter().find(|n| **n == cmd).copied() {
                    Some(view) => Action::Open { view, params: Value::Null },
                    None => {
                        app.msg = format!("Unbekannter Befehl: {cmd}");
                        Action::Stay
                    }
                }
            }
            _ => Action::Stay,
        }
    }
}

/// Inhalt der Befehlszeile, ohne Rand-Leerzeichen.
fn command_text(values: &[(FieldId, String)]) -> String {
    values
        .iter()
        .find(|(id, _)| *id == FieldId::Command)
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default()
}

fn txt(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    }
}

fn uptime(secs: u64) -> String {
    format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}

fn home(_hub: &Hub, snap: &Value, _max: usize, _scroll: usize, names: &[&'static str]) -> Vec<(String, String)> {
    let c = &snap["config"];
    vec![
        ("Modus".into(), txt(&c["mode"])),
        ("3270-Adresse".into(), txt(&c["term3270"])),
        ("Laufzeit".into(), uptime(snap["server"]["uptime_s"].as_u64().unwrap_or(0))),
        ("PID".into(), txt(&snap["server"]["pid"])),
        ("Anfragen gesamt".into(), txt(&snap["requests"]["total"])),
        ("Fehler 5xx".into(), txt(&snap["requests"]["errors_5xx"])),
        ("Log-Eintraege".into(), txt(&snap["logs"]["count"])),
        ("Seiten".into(), names.join(", ")),
    ]
}

/// Log-Fenster: `scroll` Eintraege vom neuesten Ende aus.
fn logs(hub: &Hub, _snap: &Value, max: usize, scroll: usize, _names: &[&'static str]) -> Vec<(String, String)> {
    hub.logs_window(scroll, max)
        .into_iter()
        .map(|e| {
            let secs = (e.ts_ms / 1000) % 86_400;
            let ts = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
            (format!("{ts} {:<5}", e.level), format!("{}: {}", e.target, e.message))
        })
        .collect()
}

fn ext(_hub: &Hub, snap: &Value, _max: usize, _scroll: usize, _names: &[&'static str]) -> Vec<(String, String)> {
    match snap["extensions"].as_object() {
        Some(map) if !map.is_empty() => map.iter().map(|(k, v)| (k.clone(), v.to_string())).collect(),
        _ => vec![(String::new(), "Keine Erweiterung liefert Status.".into())],
    }
}

fn config_page(_hub: &Hub, snap: &Value, _max: usize, _scroll: usize, _names: &[&'static str]) -> Vec<(String, String)> {
    snap["config"]
        .as_object()
        .map(|map| map.iter().map(|(k, v)| (k.clone(), txt(v))).collect())
        .unwrap_or_default()
}

/// Ansicht fuer einen JSON-Abschnitt einer Erweiterung: Schluessel/Wert-Zeilen,
/// verschachtelte Objekte werden mit Punkt flach dargestellt. Nur lesend.
struct SectionPage {
    title: &'static str,
    source: Arc<dyn Fn() -> Value + Send + Sync>,
}

#[async_trait]
impl View<Hub> for SectionPage {
    fn render(&self, app: &App<Hub>) -> Screen {
        let (rows, cols) = (app.rows, app.cols);
        let mut s = Screen::new(rows, cols);
        s.text(0, 0, self.title, FieldAttribute::protected_intensified(), Some(Color::Blue));
        let max = (rows as usize).saturating_sub(4);
        for (i, (label, value)) in flatten((self.source)()).iter().take(max).enumerate() {
            let row = 2 + i as u16;
            s.text(row, 0, label, FieldAttribute::normal(), Some(Color::Blue));
            s.text(row, VAL_COL, value, FieldAttribute::normal(), Some(Color::Green));
        }
        s.text(
            rows.saturating_sub(1),
            0,
            "PF3=Zurueck  PF12=Ende",
            FieldAttribute::normal(),
            Some(Color::Blue),
        );
        s
    }

    async fn handle(&mut self, _app: &mut App<Hub>, aid: Aid, _values: &[(FieldId, String)]) -> Action {
        match aid {
            Aid::Pf(3) => Action::Back,
            Aid::Pf(12) => Action::Exit,
            _ => Action::Stay,
        }
    }
}

fn flatten(v: Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Value::Object(map) = v {
        for (k, val) in map {
            match val {
                Value::Object(inner) => {
                    for (k2, v2) in inner {
                        out.push((format!("{k}.{k2}"), txt(&v2)));
                    }
                }
                other => out.push((k, txt(&other))),
            }
        }
    }
    out
}

/// Fabrik fuer eine Abschnittsansicht. `source` liefert bei jedem Aufruf
/// den aktuellen JSON-Stand. Gedacht fuer [`Extension::status_views`].
pub fn section_view(
    title: &'static str,
    source: impl Fn() -> Value + Send + Sync + 'static,
) -> ViewFactory {
    let source: Arc<dyn Fn() -> Value + Send + Sync> = Arc::new(source);
    Box::new(move || {
        Box::new(SectionPage {
            title,
            source: source.clone(),
        })
    })
}

/// Startet den 3270-Listener als eigene Task, falls `TERM3270_PORT` gesetzt ist.
pub fn spawn(config: &ServerConfig, state: &AppState) {
    let Some(port) = config.term3270_port else {
        return;
    };
    let addr = SocketAddr::new(config.term3270_bind, port);
    let exts = state.extensions.clone();

    let mut names: Vec<&'static str> = vec!["home", "logs", "ext", "config"];
    let mut ext_views: Vec<(&'static str, SharedFactory)> = Vec::new();
    for e in exts.iter() {
        for (name, factory) in e.status_views() {
            names.push(name);
            ext_views.push((name, Arc::from(factory)));
        }
    }
    let names = Arc::new(names);

    let page = |title: &'static str, content: Content, paged: bool| {
        let (names, exts) = (names.clone(), exts.clone());
        move || {
            DynView(Box::new(Page {
                title: title.to_string(),
                content,
                names: names.clone(),
                exts: exts.clone(),
                paged,
                scroll: 0,
            }))
        }
    };

    let mut server = Server::new(hub(), "home")
        .view("home", page("Startseite", home, false))
        .view("logs", page("Log-Ringbuffer", logs, true))
        .view("ext", page("Erweiterungen", ext, false))
        .view("config", page("Konfiguration", config_page, false));
    for (name, factory) in ext_views {
        server = server.view(name, move || DynView((*factory)()));
    }

    tokio::spawn(async move {
        if let Err(e) = server.serve(addr).await {
            error!(error = %format!("{e:#}"), "3270-Schnittstelle beendet mit Fehler");
        }
    });
    info!(%addr, "3270-Schnittstelle gestartet");
}
