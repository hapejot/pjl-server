//! Datei-Kern des Designers: `ddic.json` lesen und atomar zurueckschreiben.
//!
//! Der Designer ist ein Modus der Laufzeit und arbeitet ausschliesslich auf
//! dieser Datei. Kein Skript, keine Datenbank, kein Entwurfszustand.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use spec::data_dictionary::DataDictionary;

/// Liest `ddic.json`.
pub fn load(path: &Path) -> Result<DataDictionary> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read '{}'", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("'{}' is not a valid data dictionary", path.display()))
}

/// Schreibt `ddic.json` atomar: erst Temp-Datei im selben Verzeichnis, dann Rename.
/// Ein Abbruch hinterlaesst also nie eine halb geschriebene Datei.
pub fn save<T: serde::Serialize>(path: &Path, ddic: &T) -> Result<()> {
    let text = serde_json::to_string_pretty(ddic).context("cannot serialise dictionary")?;
    let tmp: PathBuf = path.with_extension("json.tmp");
    fs::write(&tmp, format!("{text}\n"))
        .with_context(|| format!("cannot write '{}'", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("cannot replace '{}'", path.display()))?;
    Ok(())
}
