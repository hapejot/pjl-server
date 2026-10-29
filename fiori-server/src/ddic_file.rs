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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    const POC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../poc-crm/ddic.json");

    #[test]
    fn poc_crm_roundtrip_is_semantically_identical() {
        let dir = std::env::temp_dir().join(format!("ddic-file-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ddic.json");

        let ddic = load(Path::new(POC)).expect("load poc-crm");
        save(&path, &ddic).expect("save");
        let again = load(&path).expect("reload");

        let before: Value = serde_json::from_str(&fs::read_to_string(POC).unwrap()).unwrap();
        let after: Value = serde_json::to_value(&again).unwrap();
        assert_eq!(before, after);
        assert!(!dir.join("ddic.json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
