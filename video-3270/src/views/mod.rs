//! Views (Bildschirme) einer Sitzung.
//!
//! Ein View kapselt Darstellung und Tastenbehandlung. Seine Daten liegen im
//! gemeinsamen Datenkontext ([`App::ctx`]). Ein neuer Screen braucht ein Modul,
//! eine `View`-Implementierung und einen Eintrag in [`create`].

mod detail;
mod list;

use async_trait::async_trait;
use serde_json::Value;

use crate::context::Context;
use crate::odata::Odata;
use crate::screen::{FieldId, Screen};
use crate::tn3270::Aid;
use detail::DetailView;
use list::ListView;

const P_ITEMS: &str = "/list/items";

/// Zustand, den alle Views einer Sitzung gemeinsam nutzen.
pub struct App {
    pub odata: Odata,
    pub ctx: Context,
    pub msg: String,
    pub rows: u16,
    pub cols: u16,
}

impl App {
    fn items(&self) -> &[Value] {
        self.ctx
            .get(P_ITEMS)
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }
}

/// Ergebnis einer Taste: was der Navigationsstack als Nächstes tun soll.
pub enum Action {
    Stay,
    /// Neuen View mit Übergabeparametern auf den Stack legen.
    Open {
        view: &'static str,
        params: Value,
    },
    /// Zum vorherigen View zurückkehren. Im Startview beendet es die Sitzung.
    Back,
    Exit,
}

#[async_trait]
pub trait View: Send {
    /// Wird beim Öffnen aufgerufen. Ein Fehler verhindert das Öffnen.
    async fn enter(&mut self, _app: &mut App, _params: Value) -> Result<(), String> {
        Ok(())
    }

    fn render(&self, app: &App) -> Screen;

    async fn handle(&mut self, app: &mut App, aid: Aid, values: &[(FieldId, String)]) -> Action;
}

/// Registry: Name -> View.
pub fn create(name: &str) -> Option<Box<dyn View>> {
    match name {
        "list" => Some(Box::new(ListView::default())),
        "detail" => Some(Box::new(DetailView)),
        _ => None,
    }
}

fn val<'a>(values: &'a [(FieldId, String)], id: FieldId) -> Option<&'a str> {
    values
        .iter()
        .find(|(f, _)| *f == id)
        .map(|(_, v)| v.as_str())
}

fn fmt_duration(secs: Option<f64>) -> String {
    match secs {
        Some(s) => {
            let s = s.round() as u64;
            format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
        }
        None => "-".into(),
    }
}

fn fmt_size(bytes: Option<i64>) -> String {
    match bytes {
        Some(b) if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        Some(b) if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        Some(b) => format!("{b} B"),
        None => "-".into(),
    }
}

fn fmt_ts(ts: Option<&str>) -> String {
    match ts {
        Some(t) => t.replace('T', " ").chars().take(16).collect(),
        None => "-".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_knows_registered_screens() {
        assert!(create("list").is_some());
        assert!(create("detail").is_some());
        assert!(create("unbekannt").is_none());
    }

    #[test]
    fn formats_values() {
        assert_eq!(fmt_duration(Some(3723.4)), "1:02:03");
        assert_eq!(fmt_size(Some(1 << 20)), "1.0 MB");
        assert_eq!(fmt_ts(Some("2024-01-02T10:00:00Z")), "2024-01-02 10:00");
        assert_eq!(fmt_ts(None), "-");
    }
}
