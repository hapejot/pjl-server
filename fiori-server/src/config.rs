//! Laufzeitkonfiguration der beiden Server.
//!
//! Beide Binaries (`pjl-designer`, `pjl-runtime`) teilen sich denselben
//! Code; sie unterscheiden sich ausschliesslich ueber [`ServerMode`] und die
//! daraus abgeleiteten Defaults. Alles ist per CLI-Flag oder Umgebungsvariable
//! ueberschreibbar - vorher waren Port, Datenverzeichnis, Schemadatei und
//! Web-Root hart verdrahtet, was insbesondere im Container nicht funktioniert
//! hat (`ENV PORT=8080` wurde ignoriert).

use std::net::IpAddr;
use std::path::PathBuf;

use clap::Parser;

/// Welche Rolle der Prozess spielt.
///
/// Die Rolle wird vom Binary festgelegt, nicht vom Benutzer - ein Designer
/// laesst sich nicht per Flag in einen Laufzeitserver verwandeln.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerMode {
    /// Pflegt das Data Dictionary: Metamodell-Store, Export-Aktion, keine
    /// fachlichen Tabellen.
    Designer,
    /// Fuehrt ein exportiertes Dictionary aus: fachliche Tabellen, kein
    /// Metamodell, kein Export.
    Runtime,
}

impl ServerMode {
    pub fn is_designer(self) -> bool {
        self == ServerMode::Designer
    }

    /// Prozessname fuer Logs und die Titelzeile des Launchpads.
    pub fn label(self) -> &'static str {
        match self {
            ServerMode::Designer => "designer",
            ServerMode::Runtime => "runtime",
        }
    }

    /// `(Port, Datenverzeichnis)` - die einzigen Defaults, die sich zwischen
    /// den beiden Servern unterscheiden. Getrennte Werte, damit beide
    /// parallel auf demselben Rechner laufen koennen.
    fn defaults(self) -> (u16, &'static str) {
        match self {
            ServerMode::Designer => (3033, "design-data"),
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

    /// Data Dictionary (JSON). Im Laufzeitmodus die Pflichteingabe, im
    /// Designer ungenutzt (dort ist das Metamodell eincompiliert).
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

    /// Zielverzeichnis der Export-Aktion (nur Designer).
    #[arg(long, env = "EXPORT_DIR", default_value = "export")]
    pub export_dir: PathBuf,

    /// Das Metamodell (Entity/Field/Relation/Package/App und dessen Apps)
    /// mitexportieren. Standardmaessig bleibt es im Designer.
    #[arg(long, env = "EXPORT_METAMODEL")]
    pub export_metamodel: bool,

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
}

impl Cli {
    pub fn into_config(self, mode: ServerMode) -> ServerConfig {
        let (default_port, default_data_dir) = mode.defaults();
        let ddic = match mode {
            ServerMode::Runtime => Some(
                self.ddic
                    .unwrap_or_else(|| self.export_dir.join("ddic.json")),
            ),
            ServerMode::Designer => self.ddic,
        };
        let ddic_overlay = match mode {
            ServerMode::Runtime => self.ddic_overlay,
            ServerMode::Designer => None,
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
            export_dir: self.export_dir,
            export_metamodel: self.export_metamodel,
            auth_user_header: self.auth_user_header,
            auth_required: self.auth_required,
            store_pg_url: self.store_pg_url,
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
    pub export_dir: PathBuf,
    pub export_metamodel: bool,
    pub auth_user_header: String,
    pub auth_required: bool,
    /// Ziel-Datenbank des Stores. `None`: SQLite in `data_dir`.
    pub store_pg_url: Option<String>,
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
            export_dir: "export".into(),
            export_metamodel: false,
            auth_user_header: "X-Forwarded-User".into(),
            auth_required: false,
            store_pg_url: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli(args: &[&str]) -> Cli {
        let mut argv = vec!["test"];
        argv.extend_from_slice(args);
        Cli::try_parse_from(argv).expect("parse")
    }

    #[test]
    fn mode_drives_the_defaults() {
        let designer = cli(&[]).into_config(ServerMode::Designer);
        assert_eq!(designer.port, 3033);
        assert_eq!(designer.data_dir, PathBuf::from("design-data"));
        // Der Designer bringt sein Dictionary selbst mit.
        assert!(designer.ddic.is_none());

        let runtime = cli(&[]).into_config(ServerMode::Runtime);
        assert_eq!(runtime.port, 3034);
        assert_eq!(runtime.data_dir, PathBuf::from("rel-data"));
        assert_eq!(runtime.ddic, Some(PathBuf::from("export/ddic.json")));
    }

    #[test]
    fn explicit_flags_win() {
        let c = cli(&["--port", "9000", "--data-dir", "/tmp/db"]).into_config(ServerMode::Runtime);
        assert_eq!(c.port, 9000);
        assert_eq!(c.data_dir, PathBuf::from("/tmp/db"));
    }

    #[test]
    fn runtime_ddic_follows_the_export_dir() {
        let c = cli(&["--export-dir", "dist"]).into_config(ServerMode::Runtime);
        assert_eq!(c.ddic, Some(PathBuf::from("dist/ddic.json")));
    }
}
