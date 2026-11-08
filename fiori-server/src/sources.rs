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

use anyhow::{Context, bail};
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
                bail!(
                    "MODEL_ROOT ist gesetzt: SCRIPTS_DIR und EXT_DIR entfallen (Layout: scripts/, ext/)"
                );
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
    let ddic = config.ddic.clone().context("designer needs --ddic")?;
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
mod tests;
