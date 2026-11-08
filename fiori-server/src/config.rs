//! Laufzeitkonfiguration der beiden Server.
//!
//! Der Laufzeitserver (`pjl-runtime`, optional `--designer`) nutzt denselben
//! Code; sie unterscheiden sich ausschliesslich ueber [`ServerMode`] und die
//! daraus abgeleiteten Defaults. Alles ist per CLI-Flag oder Umgebungsvariable
//! ueberschreibbar - vorher waren Port, Datenverzeichnis, Schemadatei und
//! Web-Root hart verdrahtet, was insbesondere im Container nicht funktioniert
//! hat (`ENV PORT=8080` wurde ignoriert).

use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;

/// Betriebsart. Es gibt nur den Laufzeitserver; der Designer ist ein Flag
/// (`--designer`), das dieselbe Laufzeit auf `ddic.json` schreibend betreibt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMode {
    Runtime,
}

impl ServerMode {
    /// Prozessname fuer Logs und Status.
    pub fn label(self) -> &'static str {
        match self {
            ServerMode::Runtime => "runtime",
        }
    }

    /// `(Port, Datenverzeichnis)` fuer den Laufzeitserver.
    fn defaults(self) -> (u16, &'static str) {
        match self {
            ServerMode::Runtime => (3034, "rel-data"),
        }
    }
}

/// Rohe Kommandozeile. Alle Werte sind optional, damit modusabhaengige
/// Defaults erst in [`Cli::into_config`] greifen koennen - clap kann nur
/// statische Defaults.
#[derive(Debug, Parser)]
#[command(about, long_about = None)]
pub struct Cli {
    /// TCP-Port des HTTP-Servers.
    #[arg(long, env = "PORT")]
    pub port: Option<u16>,

    /// Interface, auf dem gelauscht wird.
    #[arg(long, env = "BIND", default_value = "0.0.0.0")]
    pub bind: IpAddr,

    /// Verzeichnis der SQLite-Datenbank (`<dir>/data.sqlite`).
    #[arg(long, env = "DATA_DIR")]
    pub data_dir: Option<PathBuf>,

    /// Verzeichnis mit den statischen Web-Assets.
    #[arg(long, env = "WEB_ROOT", default_value = "web")]
    pub web_root: PathBuf,

    /// Data Dictionary (JSON). Pflicht im Laufzeitmodus; im Designer die
    /// Datei, die gepflegt wird.
    #[arg(long, env = "DDIC")]
    pub ddic: Option<PathBuf>,

    /// Optionales Overlay zur `ddic.json` (nur Laufzeit). Ergaenzt oder
    /// ueberschreibt Eigenschaften einzelner Entitaeten, siehe `overlay.rs`.
    #[arg(long, env = "DDIC_OVERLAY")]
    pub ddic_overlay: Option<PathBuf>,

    /// Vorgegebenes Speicher-Schema (JSON). Ohne Angabe wird das Schema aus
    /// dem Dictionary abgeleitet, inklusive Migration neuer Tabellen.
    #[arg(long, env = "SCHEMA")]
    pub schema: Option<PathBuf>,

    /// Designer-Modus: pflegt `ddic.json` ueber die Metamodell-Oberflaeche.
    /// Ohne Skripte und ohne Datenbank; jede Aenderung wird sofort geschrieben.
    #[arg(long, env = "DESIGNER", value_parser = clap::builder::BoolishValueParser::new())]
    pub designer: bool,

    /// Header, den ein vorgeschalteter Auth-Proxy (z. B. oauth2-proxy) mit dem
    /// angemeldeten Benutzer setzt. Der Proxy muss diesen Header von Clients
    /// entfernen bzw. ueberschreiben, sonst kann ihn jeder selbst setzen.
    #[arg(long, env = "AUTH_USER_HEADER", default_value = "X-Forwarded-User")]
    pub auth_user_header: String,

    /// Ohne Benutzer-Header antwortet der Server mit 401. Standard: aus, damit
    /// Entwicklung ohne Proxy moeglich bleibt.
    #[arg(long, env = "AUTH_REQUIRED")]
    pub auth_required: bool,

    /// PostgreSQL-Verbindung fuer den Store (`postgres://user:pw@host/db`).
    /// Ohne Angabe laeuft der SQLite-Store in `DATA_DIR`.
    #[arg(long, env = "STORE_PG_URL")]
    pub store_pg_url: Option<String>,

    /// Port der 3270-Terminal-Schnittstelle (Statusansichten). Ohne Angabe aus.
    #[arg(long, env = "TERM3270_PORT")]
    pub term3270_port: Option<u16>,

    /// Interface der 3270-Schnittstelle. Im Container `0.0.0.0` setzen und
    /// den Zugriff ueber das Port-Mapping steuern.
    #[arg(long, env = "TERM3270_BIND", default_value = "127.0.0.1")]
    pub term3270_bind: IpAddr,

    /// Verzeichnis mit Rhai-Skripten `<set>.rhai` (Hooks fuer Lesen und Schreiben).
    /// Ohne Angabe keine Skripte. Wird im Container als Volume eingebunden.
    #[arg(long, env = "SCRIPTS_DIR")]
    pub scripts_dir: Option<PathBuf>,

    /// Verzeichnis mit Assets je App: `<ext-dir>/<app>/` wird unter `/ext/<app>/` ausgeliefert.
    /// Ohne Angabe keine App-Assets.
    #[arg(long, env = "EXT_DIR")]
    pub ext_dir: Option<PathBuf>,

    /// Quelle der statischen Web-Assets: `file://<pfad>` oder `zip://<pfad>`.
    /// Ohne Angabe gilt `WEB_ROOT`.
    #[arg(long, env = "FIORI_ROOT")]
    pub fiori_root: Option<String>,

    /// Quelle fuer Modell, Skripte und Extensions: `file://<pfad>` oder `zip://<pfad>`.
    /// Layout: `ddic.json`, `scripts/`, `ext/` im Wurzelverzeichnis. `DDIC` und
    /// `DDIC_OVERLAY` sind dann relativ zur Wurzel.
    #[arg(long, env = "MODEL_ROOT")]
    pub model_root: Option<String>,

    /// Verzeichnis des Designers (nur `file://`). `DDIC` ist relativ dazu.
    #[arg(long, env = "DESIGNER_ROOT")]
    pub designer_root: Option<String>,
}

impl Cli {
    pub fn into_config(self, mode: ServerMode) -> ServerConfig {
        let (default_port, default_data_dir) = mode.defaults();
        // Mit MODEL_ROOT liegt das Modell im Wurzelverzeichnis der Quelle.
        let ddic = self.ddic.or_else(|| Some(PathBuf::from("ddic.json")));
        // Overlay gilt nur fuer die Laufzeit; der Designer schreibt die Datei selbst.
        let ddic_overlay = if self.designer {
            None
        } else {
            self.ddic_overlay
        };
        ServerConfig {
            mode,
            port: self.port.unwrap_or(default_port),
            bind: self.bind,
            data_dir: self.data_dir.unwrap_or_else(|| default_data_dir.into()),
            web_root: self.web_root,
            ddic,
            ddic_overlay,
            schema: self.schema,
            designer: self.designer,
            auth_user_header: self.auth_user_header,
            auth_required: self.auth_required,
            store_pg_url: self.store_pg_url,
            term3270_port: self.term3270_port,
            term3270_bind: self.term3270_bind,
            scripts_dir: self.scripts_dir,
            ext_dir: self.ext_dir,
            fiori_root: self.fiori_root,
            model_root: self.model_root,
            designer_root: self.designer_root,
        }
    }
}

/// Aufgeloeste Konfiguration; ab hier gibt es keine Optionen mehr, die der
/// Server erst noch interpretieren muesste.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub mode: ServerMode,
    pub port: u16,
    pub bind: IpAddr,
    pub data_dir: PathBuf,
    pub web_root: PathBuf,
    pub ddic: Option<PathBuf>,
    /// Overlay zur `ddic.json`; nur im Laufzeitmodus wirksam.
    pub ddic_overlay: Option<PathBuf>,
    pub schema: Option<PathBuf>,
    /// Designer-Modus (siehe `Cli::designer`).
    pub designer: bool,
    pub auth_user_header: String,
    pub auth_required: bool,
    /// Ziel-Datenbank des Stores. `None`: SQLite in `data_dir`.
    pub store_pg_url: Option<String>,
    /// 3270-Schnittstelle nur aktiv, wenn gesetzt.
    pub term3270_port: Option<u16>,
    pub term3270_bind: IpAddr,
    /// Skriptverzeichnis; `None` schaltet die Skript-Hooks aus.
    pub scripts_dir: Option<PathBuf>,
    /// Asset-Verzeichnis je App (`<ext_dir>/<app>/`); `None` schaltet sie aus.
    pub ext_dir: Option<PathBuf>,
    /// Quelle der Web-Assets (`file://` oder `zip://`); `None`: `web_root`.
    pub fiori_root: Option<String>,
    /// Quelle fuer Modell, Skripte und Extensions; `None`: Einzelpfade.
    pub model_root: Option<String>,
    /// Designer-Verzeichnis (nur `file://`).
    pub designer_root: Option<String>,
}

impl ServerConfig {
    /// Konfiguration fuer das angegebene Binary aus der Kommandozeile lesen.
    pub fn parse(mode: ServerMode) -> Self {
        Cli::parse().into_config(mode)
    }

    /// Reine Defaults, ohne Kommandozeile und ohne Umgebung.
    ///
    /// Fuer Tests und eingebettete Verwendung; der Laufzeitmodus bekommt hier
    /// bewusst kein Dictionary zugewiesen, das muss der Aufrufer setzen.
    pub fn defaults(mode: ServerMode) -> Self {
        let (port, data_dir) = mode.defaults();
        Self {
            mode,
            port,
            bind: IpAddr::from([0, 0, 0, 0]),
            data_dir: data_dir.into(),
            web_root: "web".into(),
            ddic: None,
            ddic_overlay: None,
            schema: None,
            designer: false,
            auth_user_header: "X-Forwarded-User".into(),
            auth_required: false,
            store_pg_url: None,
            term3270_port: None,
            term3270_bind: IpAddr::from([127, 0, 0, 1]),
            scripts_dir: None,
            ext_dir: None,
            fiori_root: None,
            model_root: None,
            designer_root: None,
        }
    }
}

#[cfg(test)]
mod tests;
