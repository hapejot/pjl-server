//! Datei-Kern des Designers: Dictionaries lesen und zurueckschreiben.
//!
//! Der Designer ist ein Modus der Laufzeit und arbeitet ausschliesslich auf
//! der Hauptdatei und ihren Paketdateien. Kein Skript, keine Datenbank, kein Entwurfszustand.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use file_repo::DirRepository;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use spec::data_dictionary::DataDictionary;

use crate::ddic_packages::{self, Layout};

/// Liest `ddic.json`.
pub fn load(path: &Path) -> Result<DataDictionary> {
    let (value, _) = load_files(path, false)?;
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

fn journal_path(path: &Path) -> Result<PathBuf> {
    Ok(parent(path).join(".ddic-packages.transaction"))
}

/// Laedt alle Paketdateien unter einer gemeinsamen Sperre.
/// Der Designer stellt unterbrochene Transaktionen vor dem Lesen wieder her.
pub fn load_files(path: &Path, recover: bool) -> Result<(Value, Option<Layout>)> {
    let dir = parent(path);
    let lock = File::open(dir).with_context(|| format!("cannot open '{}'", dir.display()))?;
    if recover {
        lock.lock()?;
    } else {
        lock.lock_shared()?;
    }
    let journal = journal_path(path)?;
    if journal.exists() {
        if recover {
            rollback(path)?;
        } else {
            bail!(
                "Unvollstaendige DDIC-Transaktion in '{}'; Designer zur Wiederherstellung starten",
                journal.display()
            );
        }
    }
    ddic_packages::load(&DirRepository::new(dir), basename(path)?)
}

#[derive(Serialize, Deserialize)]
struct Journal {
    dictionary: String,
    files: Vec<PreviousFile>,
}

#[derive(Serialize, Deserialize)]
struct PreviousFile {
    path: String,
    content: String,
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

fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?
        .sync_all()
        .with_context(|| format!("cannot sync '{}'", path.display()))
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    let mut file =
        File::create(&tmp).with_context(|| format!("cannot write '{}'", tmp.display()))?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace '{}'", path.display()))?;
    sync_dir(parent(path))
}

fn rollback(path: &Path) -> Result<()> {
    let journal_path = journal_path(path)?;
    let journal: Journal = serde_json::from_slice(&fs::read(&journal_path)?)
        .with_context(|| format!("invalid transaction journal '{}'", journal_path.display()))?;
    ensure!(
        journal.dictionary == basename(path)?,
        "DDIC-Transaktion gehoert zu '{}'; dieses Dictionary zur Wiederherstellung oeffnen",
        journal.dictionary
    );
    ensure!(!journal.files.is_empty(), "leeres DDIC-Transaktionsjournal");
    let mut seen = std::collections::HashSet::new();
    let targets = journal
        .files
        .iter()
        .map(|file| {
            let target = checked_path(parent(path), basename(path)?, &file.path)?;
            ensure!(
                seen.insert(target.clone()),
                "doppelter Pfad im DDIC-Journal"
            );
            serde_json::from_str::<Value>(&file.content)
                .context("ungueltiger Dateiinhalt im DDIC-Journal")?;
            Ok(target)
        })
        .collect::<Result<Vec<_>>>()?;
    for (file, target) in journal.files.iter().zip(targets) {
        write_atomic(&target, &file.content)?;
    }
    fs::remove_file(&journal_path)?;
    sync_dir(parent(path))
}

/// Schreibt die geaenderten Dateien mit wiederherstellbarem Rueckroll-Journal.
pub fn save_packages(path: &Path, base: &Layout, combined: &Value) -> Result<Layout> {
    let dir = parent(path);
    let lock = File::open(dir)?;
    lock.lock()?;
    if journal_path(path)?.exists() {
        rollback(path)?;
    }
    let name = basename(path)?;
    let (_, current) = ddic_packages::load(&DirRepository::new(dir), name)?;
    let current = current.context("Paketkonfiguration wurde entfernt")?;
    if current.root != base.root || current.files != base.files {
        bail!("Dictionary oder Paketdateien wurden extern geaendert; bitte neu laden");
    }
    let output = ddic_packages::split(name, combined, base)?;
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
    if !changes.is_empty() {
        let journal = Journal {
            dictionary: name.to_string(),
            files: changes
                .iter()
                .map(|(path, content, _)| PreviousFile {
                    path: path.clone(),
                    content: content.clone(),
                })
                .collect(),
        };
        let journal_path = journal_path(path)?;
        let journal_tmp = journal_path.with_extension("transaction.tmp");
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&journal_tmp)?;
        file.write_all(&serde_json::to_vec(&journal)?)?;
        file.sync_all()?;
        fs::rename(&journal_tmp, &journal_path)?;
        if let Err(error) = sync_dir(dir) {
            rollback(path).context("DDIC-Wiederherstellung nach Journalfehler fehlgeschlagen")?;
            return Err(error);
        }
        let result = changes.iter().try_for_each(|(relative, _, content)| {
            write_atomic(&checked_path(dir, name, relative)?, content)
        });
        if let Err(error) = result {
            rollback(path).context("DDIC-Wiederherstellung nach Schreibfehler fehlgeschlagen")?;
            return Err(error);
        }
        if let Err(error) = fs::remove_file(&journal_path) {
            rollback(path).context("DDIC-Wiederherstellung nach Journalfehler fehlgeschlagen")?;
            return Err(error.into());
        }
        if let Err(error) = sync_dir(dir) {
            write_atomic(&journal_path, &serde_json::to_string(&journal)?)
                .context("DDIC-Commit ungewiss; Wiederherstellungsjournal konnte nicht erneut geschrieben werden")?;
            rollback(path)
                .context("DDIC-Wiederherstellung nach Synchronisierungsfehler fehlgeschlagen")?;
            return Err(error);
        }
    }
    Ok(updated)
}

/// Schreibt `ddic.json` atomar: erst Temp-Datei im selben Verzeichnis, dann Rename.
/// Ein Abbruch hinterlaesst also nie eine halb geschriebene Datei.
pub fn save<T: serde::Serialize>(path: &Path, ddic: &T) -> Result<()> {
    let lock = File::open(parent(path))?;
    lock.lock()?;
    if journal_path(path)?.exists() {
        bail!(
            "Unvollstaendige DDIC-Transaktion; das zugehoerige Dictionary zur Wiederherstellung im Designer oeffnen"
        );
    }
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
