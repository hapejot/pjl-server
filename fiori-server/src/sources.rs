//! Aufloesung der Quellen aus `FIORI_ROOT`, `MODEL_ROOT` und `DESIGNER_ROOT`.
//!
//! * `FIORI_ROOT`: statische Web-Assets (`file://` oder `zip://`). Ohne Angabe `WEB_ROOT`.
//! * `MODEL_ROOT`: `ddic.json`, Overlay, `scripts/` und `ext/` (`file://` oder `zip://`).
//!   `DDIC`, `DDIC_OVERLAY` werden dann relativ zur Wurzel gelesen. Ohne Angabe gelten die
//!   Einzelpfade `DDIC`, `DDIC_OVERLAY`, `SCRIPTS_DIR`, `EXT_DIR` wie bisher.
//! * `DESIGNER_ROOT`: Verzeichnis fuer den Designer, nur `file://`. `DDIC` ist relativ dazu.
//!   Der Designer schreibt ausschliesslich auf das Dateisystem.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context};
use file_repo::{DirRepository, FileRepository, Limits};

use crate::config::ServerConfig;

/// Datei innerhalb einer Quelle.
#[derive(Clone)]
pub struct FileRef {
    pub repo: Arc<dyn FileRepository>,
    pub rel: String,
    pub label: String,
}

/// Verzeichnis innerhalb einer Quelle (Skripte, Extensions).
#[derive(Clone)]
pub struct DirRef {
    pub repo: Arc<dyn FileRepository>,
    /// Dateisystempfad, falls vorhanden. Nur dann gibt es Hot-Reload.
    pub fs_dir: Option<PathBuf>,
    pub label: String,
}

/// Aufgeloeste Quellen des Servers.
pub struct Sources {
    pub fiori: Arc<dyn FileRepository>,
    pub fiori_label: String,
    pub ddic: FileRef,
    pub overlay: Option<FileRef>,
    pub scripts: Option<DirRef>,
    pub ext: Option<DirRef>,
}

impl FileRef {
    /// Einzelne Datei auf dem Dateisystem.
    fn from_path(path: &Path) -> FileRef {
        let parent = match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        };
        FileRef {
            repo: Arc::new(DirRepository::new(parent)),
            rel: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            label: path.display().to_string(),
        }
    }
}

impl DirRef {
    fn from_dir(dir: &Path) -> DirRef {
        DirRef {
            repo: Arc::new(DirRepository::new(dir)),
            fs_dir: Some(dir.to_path_buf()),
            label: dir.display().to_string(),
        }
    }
}

fn open(uri: &str) -> anyhow::Result<Arc<dyn FileRepository>> {
    let repo = file_repo::open(uri, &Limits::default())
        .with_context(|| format!("cannot open source '{uri}'"))?;
    Ok(Arc::from(repo))
}

/// Quellen aus der Konfiguration aufloesen. Fehler brechen den Start ab.
pub fn resolve(config: &ServerConfig) -> anyhow::Result<Sources> {
    let fiori: Arc<dyn FileRepository> = match &config.fiori_root {
        Some(uri) => open(uri)?,
        None => Arc::new(DirRepository::new(&config.web_root)),
    };
    let fiori_label = config
        .fiori_root
        .clone()
        .unwrap_or_else(|| config.web_root.display().to_string());

    let ddic_rel = config
        .ddic
        .clone()
        .context("no data dictionary configured (DDIC)")?;

    let (ddic, overlay, scripts, ext) = match &config.model_root {
        Some(uri) => {
            if config.scripts_dir.is_some() || config.ext_dir.is_some() {
                bail!("MODEL_ROOT ist gesetzt: SCRIPTS_DIR und EXT_DIR entfallen (Layout: scripts/, ext/)");
            }
            let repo = open(uri)?;
            let fs_root = file_repo::file_path(uri);
            let rel_of = |p: &Path| p.to_string_lossy().into_owned();
            let ddic = FileRef {
                repo: repo.clone(),
                rel: rel_of(&ddic_rel),
                label: uri.clone(),
            };
            let overlay = config.ddic_overlay.as_ref().map(|p| FileRef {
                repo: repo.clone(),
                rel: rel_of(p),
                label: uri.clone(),
            });
            let sub = |name: &str| -> anyhow::Result<Option<DirRef>> {
                if !repo.exists(name) {
                    return Ok(None);
                }
                Ok(Some(DirRef {
                    repo: file_repo::scoped(repo.clone(), name)?,
                    fs_dir: fs_root.as_ref().map(|r| r.join(name)),
                    label: format!("{uri}/{name}"),
                }))
            };
            let scripts = sub("scripts")?;
            let ext = sub("ext")?;
            (ddic, overlay, scripts, ext)
        }
        None => {
            let ddic = FileRef::from_path(&ddic_rel);
            let overlay = config.ddic_overlay.as_deref().map(FileRef::from_path);
            let scripts = config.scripts_dir.as_deref().map(DirRef::from_dir);
            let ext = config.ext_dir.as_deref().map(DirRef::from_dir);
            (ddic, overlay, scripts, ext)
        }
    };

    Ok(Sources {
        fiori,
        fiori_label,
        ddic,
        overlay,
        scripts,
        ext,
    })
}

/// Dateipfad der Designer-Datei. Der Designer schreibt nur auf das Dateisystem.
pub fn designer_path(config: &ServerConfig) -> anyhow::Result<PathBuf> {
    let ddic = config
        .ddic
        .clone()
        .context("designer needs --ddic")?;
    match &config.designer_root {
        Some(uri) => {
            let root = file_repo::file_path(uri).with_context(|| {
                format!("DESIGNER_ROOT '{uri}': der Designer schreibt nur auf file://")
            })?;
            Ok(root.join(ddic))
        }
        None => Ok(ddic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, header};
    use std::io::{Cursor, Write};

    /// Minimales Modell als ZIP im Layout `ddic.json`, `scripts/`, `ext/`.
    /// Alle Inhalte stehen im Test selbst.
    fn model_zip() -> Vec<u8> {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file("ddic.json", opts).unwrap();
            w.write_all(br#"{"entities":[],"apps":[]}"#).unwrap();
            w.start_file("scripts/CustomerSet.rhai", opts).unwrap();
            w.write_all(b"fn before_patch(ctx) { ctx }").unwrap();
            w.start_file("ext/videos/Player.js", opts).unwrap();
            w.write_all(b"var a;").unwrap();
            w.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn model_root_zip_provides_ddic_scripts_and_ext() {
        let dir = tempfile::tempdir().unwrap();
        let zp = dir.path().join("model.zip");
        std::fs::write(&zp, model_zip()).unwrap();

        let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
        config.model_root = Some(format!("zip://{}", zp.display()));
        config.ddic = Some(PathBuf::from("ddic.json"));

        let sources = resolve(&config).unwrap();
        assert!(sources.ddic.repo.exists("ddic.json"));
        assert!(sources.overlay.is_none());
        let scripts = sources.scripts.expect("scripts");
        assert!(scripts.fs_dir.is_none(), "zip hat keinen Dateisystempfad");
        let hooks = script_hooks::ScriptHooks::load_from(scripts.repo, scripts.label).unwrap();
        assert!(hooks.status()["sets"].get("CustomerSet").is_some());
        assert!(sources.ext.expect("ext").repo.exists("videos/Player.js"));
    }

    #[test]
    fn model_root_rejects_legacy_script_and_ext_dirs() {
        let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
        config.model_root = Some("zip:///nonexistent.zip".into());
        config.scripts_dir = Some(PathBuf::from("x"));
        assert!(resolve(&config).is_err());
    }

    #[test]
    fn designer_root_must_be_file() {
        let mut config = ServerConfig::defaults(crate::config::ServerMode::Runtime);
        config.designer = true;
        config.designer_root = Some("zip:///x.zip".into());
        assert!(designer_path(&config).is_err());

        config.designer_root = Some("file:///srv/designer".into());
        config.ddic = Some(PathBuf::from("ddic.json"));
        assert_eq!(designer_path(&config).unwrap(), PathBuf::from("/srv/designer/ddic.json"));
    }

    #[test]
    fn fiori_root_zip_is_served_with_content_type() {
        let mut buf = Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("index.html", opts).unwrap();
            w.write_all(b"<html></html>").unwrap();
            w.start_file("apps/x/manifest.json", opts).unwrap();
            w.write_all(b"{}").unwrap();
            w.finish().unwrap();
        }
        let repo = file_repo::ZipRepository::from_bytes(
            bytes::Bytes::from(buf.into_inner()),
            &Limits::default(),
        )
        .unwrap();
        let ok = crate::repo_serve::serve(&repo, "");
        assert_eq!(ok.status(), StatusCode::OK);
        assert_eq!(ok.headers()[header::CONTENT_TYPE], "text/html");
        let json = crate::repo_serve::serve(&repo, "/apps/x/manifest.json");
        assert_eq!(json.headers()[header::CONTENT_TYPE], "application/json");
        assert_eq!(
            crate::repo_serve::serve(&repo, "../etc/passwd").status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            crate::repo_serve::serve(&repo, "missing.js").status(),
            StatusCode::NOT_FOUND
        );
    }
}
