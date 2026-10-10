//! Export des im Designer gepflegten Modells.
//!
//! Erzeugt genau die zwei Dateien, die der Laufzeitserver beim Start liest:
//!
//! * `ddic.json`   - das semantische Dictionary (`spec::DataDictionary`)
//! * `schema.json` - das daraus abgeleitete, validierte Speicher-Schema
//!
//! Beide entstehen aus derselben Quelle, damit sie nicht auseinanderlaufen
//! koennen: erst wird das Dictionary aus dem Store gelesen, dann gefiltert,
//! dann aktiviert, und erst das Ergebnis geht durch `spec_schema`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use spec::data_dictionary::DataDictionary;
use tracing::{info, trace, warn};

use crate::AppState;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("export directory '{path}' not usable: {source}")]
    Directory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("writing '{path}' failed: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("serialisation failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("the modelled dictionary does not yield a valid schema: {0}")]
    Schema(#[from] data_store::schema::SchemaError),
}

/// Was der Export produziert hat - geht als JSON an die Oberflaeche zurueck,
/// damit der Button eine sinnvolle Rueckmeldung zeigen kann.
#[derive(Debug, Serialize)]
pub struct ExportReport {
    pub ddic_path: String,
    pub schema_path: String,
    pub packages: usize,
    pub entities: usize,
    pub relationships: usize,
    pub apps: usize,
    pub entity_sets: usize,
    /// Objekte, die als Metamodell erkannt und nicht exportiert wurden.
    pub skipped_metamodel: usize,
}

/// Modell aus dem Store lesen, filtern und als Dateipaar ablegen.
pub async fn export_dictionary(state: &AppState) -> Result<ExportReport, ExportError> {
    info!("starting export of dictionary");
    let designed = state.designed_dictionary().await;
    let (exported, skipped) = if state.config.export_metamodel {
        (designed, 0)
    } else {
        without_metamodel(&designed)
    };

    // Ableitung und Validierung passieren vor dem ersten Schreibzugriff:
    // ein unvollstaendiges Modell soll kein halbes Exportverzeichnis
    // hinterlassen, das die Laufzeit dann laedt.
    let mut activated = exported.clone();
    activated.activate();
    let schema = spec_schema::schema_from_ddic(&activated)?;

    let dir = &state.config.export_dir;
    fs::create_dir_all(dir).map_err(|source| ExportError::Directory {
        path: dir.clone(),
        source,
    })?;

    let ddic_path = dir.join("ddic.json");
    let schema_path = dir.join("schema.json");
    // Das unaktivierte Dictionary wird geschrieben: die Laufzeit ruft selbst
    // `activate()` auf, und abgeleitete Lookups duerfen nicht persistiert
    // werden, sonst stehen sie nach dem Laden doppelt drin.
    write_json(&ddic_path, &exported)?;
    write_json(&schema_path, &schema)?;

    let report = ExportReport {
        ddic_path: ddic_path.display().to_string(),
        schema_path: schema_path.display().to_string(),
        packages: exported.packages().count(),
        entities: exported.entities().len(),
        relationships: exported.relationships().count(),
        apps: exported.apps().count(),
        entity_sets: schema.sets().count(),
        skipped_metamodel: skipped,
    };
    info!(
        "exported {} entities / {} apps to {}",
        report.entities,
        report.apps,
        dir.display()
    );
    Ok(report)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), ExportError> {
    let json = serde_json::to_vec_pretty(value)?;
    fs::write(path, json).map_err(|source| ExportError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Alles wegfiltern, was zum eincompilierten Metamodell gehoert.
///
/// Der Designer verwaltet sein eigenes Modell in denselben Tabellen, in denen
/// er das Kundenmodell speichert. Ohne diesen Filter bekaeme der
/// Laufzeitserver `Entity`, `Field`, `Relation`, `Package`, `App` und die fuenf
/// Pflege-Apps mitgeliefert - er wuerde also den Designer duplizieren.
///
/// Gefiltert wird nach Namen gegen `DataDictionary::new()`, nicht nach Paket:
/// vom Anwender angelegte Objekte landen mangels Zuordnung ebenfalls im Paket
/// `core` und wuerden sonst mit verschwinden.
fn without_metamodel(source: &DataDictionary) -> (DataDictionary, usize) {
    let meta = DataDictionary::new();
    let meta_entities: Vec<String> = meta.entity_names().map(str::to_string).collect();
    info!(?meta_entities, "Meta Entities");
    let meta_apps: Vec<String> = meta.apps().map(|a| a.name().to_string()).collect();
    let meta_relations: Vec<String> = meta.relationships().map(|r| r.name().to_string()).collect();

    let mut out = DataDictionary::empty();
    let mut skipped = 0usize;

    for package in source.packages() {
        out.add_package(package.clone());
    }
    for entity in source.entities() {
        if meta_entities.iter().any(|n| n == entity.name()) {
            trace!("skipping {}", entity.name);
            skipped += 1;
            continue;
        }
        out.add_entity(entity);
    }
    // Eine Relationship ueberlebt nur, wenn beide Enden exportiert werden -
    // sonst zeigt sie ins Leere und `schema_from_ddic` scheitert am
    // unbekannten Ziel.
    let kept: Vec<String> = out.entity_names().map(str::to_string).collect();
    for rel in source.relationships() {
        if meta_relations.iter().any(|n| n == rel.name()) {
            skipped += 1;
            continue;
        }
        let ends_known =
            kept.iter().any(|n| n == rel.one()) && kept.iter().any(|n| n == rel.many());
        if !ends_known {
            warn!(
                "relationship '{}' points outside the exported model ({} -> {}) - skipped",
                rel.name(),
                rel.one(),
                rel.many()
            );
            skipped += 1;
            continue;
        }
        out.add_relationship(rel.clone());
    }
    for app in source.apps() {
        if meta_apps.iter().any(|n| n == app.name()) {
            skipped += 1;
            continue;
        }
        out.add_app(app.clone());
    }
    (out, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metamodel_is_stripped() {
        let (out, skipped) = without_metamodel(&DataDictionary::new());
        assert_eq!(out.entities().len(), 0, "nothing but the metamodel was in");
        assert_eq!(out.apps().count(), 0);
        assert!(skipped > 0);
    }

    #[test]
    fn packages_survive_so_user_entities_keep_their_home() {
        let (out, _) = without_metamodel(&DataDictionary::new());
        assert!(out.packages().any(|p| p.name() == "core"));
    }
}
