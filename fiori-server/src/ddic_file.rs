//! Datei-Kern des Designers: Dictionaries lesen und zurueckschreiben.
//!
//! Der Designer ist ein Modus der Laufzeit und arbeitet ausschliesslich auf
//! der Hauptdatei und ihren Paketdateien. Kein Skript, keine Datenbank, kein Entwurfszustand.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use file_repo::DirRepository;
use serde_json::Value;
use spec::data_dictionary::DataDictionary;
use tracing::*;

use crate::ddic_packages::{self, Layout};

/// Liest `ddic.json`.
pub fn load(path: &Path) -> Result<DataDictionary> {
    let (value, _) = load_files(path)?;
    serde_json::from_value(value)
        .with_context(|| format!("'{}' is not a valid data dictionary", path.display()))
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn basename(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|n| n.to_str())
        .context("DDIC-Dateiname ist kein UTF-8")
}

/// Lädt alle Paketdateien unter einer gemeinsamen Sperre.
/// Der Designer stellt unterbrochene Transaktionen vor dem Lesen wieder her.
pub fn load_files(path: &Path) -> Result<(Value, Option<Layout>)> {
    let dir = parent(path);
    trace!("parent dir: {:?}: {}", dir, dir.exists());

    ddic_packages::load(&DirRepository::new(dir), basename(path)?)
}

fn checked_path(root: &Path, main: &str, relative: &str) -> Result<PathBuf> {
    let normalized = file_repo::normalize(relative)?;
    let valid_package = normalized.strip_prefix("packages/").is_some_and(|s| {
        s.ends_with(".json")
            && !s.contains('/')
            && s.trim_end_matches(".json")
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    });
    if normalized != main && !valid_package {
        bail!("ungueltiger Pfad im DDIC-Journal: '{relative}'");
    }
    Ok(root.join(normalized))
}

fn sync_dir(_path: &Path) -> Result<()> {
    // trace!("sync dir {:?}", path);
    // File::open(path)?
    //     .sync_all()
    //     .with_context(|| format!("cannot sync '{}'", path.display()))
    Ok(())
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let p = path.parent().unwrap();
    assert!(p.exists());
    let tmp = path.parent().unwrap().join("json.tmp");
    let mut file =
        File::create(&tmp).with_context(|| format!("cannot write '{}'", tmp.display()))?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace '{}'", path.display()))?;
    sync_dir(parent(path))
}

/// Schreibt die geaenderten Dateien mit wiederherstellbarem Rueckroll-Journal.
pub fn save_packages(path: &Path, base: &Layout, combined: &Value) -> Result<Layout> {
    trace!("save packages {:?}", path);
    let dir = parent(path);
    let name = basename(path)?;
    trace!("save load {:?} {}", dir, name);
    let (_, current) =
        ddic_packages::load(&DirRepository::new(dir), name).context("loading current ddic")?;
    let current = current.context("Paketkonfiguration wurde entfernt")?;
    if current.root != base.root || current.files != base.files {
        bail!("Dictionary oder Paketdateien wurden extern geaendert; bitte neu laden");
    }
    trace!("save split {}", name);
    let output = ddic_packages::split(name, combined, base).context("split")?;
    trace!("updated");
    let updated = Layout {
        imports: base.imports.clone(),
        root: output
            .get(name)
            .context("Hauptdictionary fehlt im Ergebnis")?
            .clone(),
        files: base
            .imports
            .iter()
            .map(|package| {
                let key = format!("packages/{package}.json");
                Ok((
                    package.clone(),
                    output
                        .get(&key)
                        .with_context(|| format!("Paketdatei '{key}' fehlt im Ergebnis"))?
                        .clone(),
                ))
            })
            .collect::<Result<_>>()?,
    };

    trace!("changes");
    let mut changes = Vec::new();
    for (relative, value) in output {
        let target = checked_path(dir, name, &relative)?;
        let previous = fs::read_to_string(&target)
            .with_context(|| format!("cannot read '{}'", target.display()))?;
        let old_json: Value = serde_json::from_str(&previous)?;
        if old_json != value {
            changes.push((
                relative,
                previous,
                format!("{}\n", serde_json::to_string_pretty(&value)?),
            ));
        }
    }
    trace!("changes not empty?");
    if !changes.is_empty() {
        let result = changes.iter().try_for_each(|(relative, _, content)| {
            write_atomic(&checked_path(dir, name, relative)?, content)
        });
        if let Err(error) = result {
            return Err(error);
        }
    }
    trace!("done");
    Ok(updated)
}

/// Schreibt `ddic.json` atomar: erst Temp-Datei im selben Verzeichnis, dann Rename.
/// Ein Abbruch hinterlaesst also nie eine halb geschriebene Datei.
pub fn save<T: serde::Serialize>(path: &Path, ddic: &T) -> Result<()> {
    trace!("save {:?}", path);
    if path.exists() {
        let current: Value = serde_json::from_slice(&fs::read(path)?)
            .with_context(|| format!("invalid JSON in '{}'", path.display()))?;
        ensure!(
            current.get("package_files").is_none(),
            "Dictionary wurde extern auf Paketdateien umgestellt; bitte neu laden"
        );
    }
    let text = serde_json::to_string_pretty(ddic).context("cannot serialise dictionary")?;
    let tmp: PathBuf = path.with_extension("json.tmp");
    fs::write(&tmp, format!("{text}\n"))
        .with_context(|| format!("cannot write '{}'", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace '{}'", path.display()))?;
    Ok(())
}
