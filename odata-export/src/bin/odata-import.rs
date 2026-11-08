//! Spielt einen mit `odata-export` (oder `scripts/odata-export.py`) erzeugten
//! Datenbestand auf einen OData-Service zurueck. Gegenstueck zu `odata-export`,
//! dasselbe Verhalten wie `scripts/odata-import.py`:
//!
//!   * Sets in der Reihenfolge, die Fremdschluessel verlangen.
//!   * Je Satz: Schluessel vorhanden -> PATCH, sonst POST (aktiv, ohne Entwurf).
//!   * GUIDs bleiben erhalten. Fremdschluessel, die auf dem Ziel anders heissen
//!     (`status_ID` GUID -> `status_code` mit Code-Schluessel), werden ueber den
//!     Quelldatensatz uebersetzt.
//!   * Fehler je Satz werden gesammelt; Exit-Code 1 bei Fehlern.
//!
//!     odata-import --base-url http://localhost:8080/Service --from DIR [--dry-run]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::{Map, Value};

use odata_export::http::Client;
use odata_export::model::{EntityType, Model};
use odata_export::{DRAFT_PROPERTIES, FORMAT_VERSION};

const MAX_REPORTED_ERRORS: usize = 20;

#[derive(Parser, Debug)]
#[command(about = "Spielt einen Export auf einen OData-Service zurueck")]
struct Args {
    /// Service-Wurzel des Ziels, z. B. http://localhost:8080/Service (nur http://)
    #[arg(long)]
    base_url: String,
    /// Verzeichnis aus odata-export
    #[arg(long = "from")]
    source: PathBuf,
    /// Nur pruefen und uebersetzen, nichts schreiben
    #[arg(long)]
    dry_run: bool,
}

#[derive(Default)]
struct SetStats {
    created: usize,
    updated: usize,
    unchanged: usize,
    errors: usize,
}

struct Report {
    per_set: Vec<(String, SetStats)>,
    errors: Vec<String>,
}

impl Report {
    fn stats(&mut self, set: &str) -> &mut SetStats {
        if let Some(i) = self.per_set.iter().position(|(s, _)| s == set) {
            return &mut self.per_set[i].1;
        }
        self.per_set.push((set.to_string(), SetStats::default()));
        &mut self.per_set.last_mut().unwrap().1
    }

    fn error(&mut self, set: &str, message: String) {
        self.stats(set).errors += 1;
        self.errors.push(format!("{set}: {message}"));
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Fehler: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let manifest: Value = read_json(&args.source.join("manifest.json"))?;
    if manifest.get("format").and_then(Value::as_u64) != Some(FORMAT_VERSION) {
        bail!("unbekanntes Exportformat: {}", manifest.get("format").unwrap_or(&Value::Null));
    }
    let sources = source_sets(&manifest)?;

    let client = Client::new(&args.base_url)?;
    let (status, xml) = client.get("/$metadata")?;
    if status != 200 {
        bail!("$metadata des Ziels lieferte HTTP {status}");
    }
    let model = Model::parse(&String::from_utf8(xml).context("$metadata ist kein UTF-8")?)?;

    let order_names: Vec<String> = manifest["order"]
        .as_array()
        .context("manifest.order fehlt")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let (sets, skipped): (Vec<String>, Vec<String>) =
        order_names.into_iter().partition(|s| model.sets.contains_key(s));
    for s in &skipped {
        println!("uebersprungen (Ziel kennt das Set nicht): {s}");
    }
    let order = model.order(&sets)?;

    // Quelldaten je Set als Index ID -> Satz, fuer die FK-Uebersetzung.
    let mut source_index: HashMap<String, HashMap<String, Map<String, Value>>> = HashMap::new();
    for set in &sets {
        let records = read_records(&args.source, &sources[set])?;
        let index = records
            .into_iter()
            .filter_map(|r| r.get("ID").map(|id| (value_to_key(id), r.clone())))
            .filter_map(|(id, r)| r.as_object().cloned().map(|o| (id, o)))
            .collect();
        source_index.insert(set.clone(), index);
    }

    let mut report = Report { per_set: Vec::new(), errors: Vec::new() };
    for set in &order {
        let etype = model.set_type(set).context("Typ fehlt")?.clone();
        let records = read_records(&args.source, &sources[set])?;
        for record in records {
            let Some(record) = record.as_object() else { continue };
            import_record(&client, &model, &etype, set, record, &source_index, &mut report, args.dry_run);
        }
        let s = report.stats(set);
        println!(
            "{set:32} neu {:5}  aktualisiert {:5}  unveraendert {:5}  Fehler {:4}",
            s.created, s.updated, s.unchanged, s.errors
        );
    }

    let total_errors: usize = report.per_set.iter().map(|(_, s)| s.errors).sum();
    let mode = if args.dry_run { "Probelauf" } else { "Import" };
    println!("{mode} abgeschlossen, {total_errors} Fehler");
    for message in report.errors.iter().take(MAX_REPORTED_ERRORS) {
        println!("  {message}");
    }
    if report.errors.len() > MAX_REPORTED_ERRORS {
        println!("  ... {} weitere", report.errors.len() - MAX_REPORTED_ERRORS);
    }
    if total_errors > 0 {
        std::process::exit(1);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn import_record(
    client: &Client,
    model: &Model,
    etype: &EntityType,
    set: &str,
    record: &Map<String, Value>,
    source_index: &HashMap<String, HashMap<String, Map<String, Value>>>,
    report: &mut Report,
    dry_run: bool,
) {
    let mut body: Map<String, Value> = record
        .iter()
        .filter(|(k, _)| etype.properties.contains(k) && !DRAFT_PROPERTIES.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let satz = record.get("ID").map(value_to_key).unwrap_or_else(|| "?".into());
    if let Err(message) = translate_foreign_keys(model, etype, record, source_index, &mut body) {
        report.error(set, format!("{satz}: {message}"));
        return;
    }
    let business = etype.business_keys();
    if business.iter().any(|k| !body.contains_key(*k)) {
        report.error(set, format!("{satz}: Schluessel {business:?} fehlt im Satz"));
        return;
    }
    let outcome = write_record(client, etype, set, &body, dry_run);
    match outcome {
        Ok(Write::Created) => report.stats(set).created += 1,
        Ok(Write::Updated) => report.stats(set).updated += 1,
        Ok(Write::Unchanged) => report.stats(set).unchanged += 1,
        Err(err) => report.error(set, format!("{satz}: {err:#}")),
    }
}

enum Write {
    Created,
    Updated,
    /// Der Satz existiert mit identischen Werten, es wird nichts geschrieben.
    Unchanged,
}

fn write_record(
    client: &Client,
    etype: &EntityType,
    set: &str,
    body: &Map<String, Value>,
    dry_run: bool,
) -> Result<Write> {
    let key = etype.key_expr(&Value::Object(body.clone()), true)?;
    let path = format!("/{set}({key})");
    let (status, existing) = client.get(&path)?;
    if status == 200 {
        let business = etype.business_keys();
        let patch: Map<String, Value> = body
            .iter()
            .filter(|(k, _)| !business.contains(&k.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Nur Abweichungen schreiben. Sonst setzt der PATCH bei auditierten Entitaeten
        // ChangedAt/ChangedBy auf Werte, die sich inhaltlich nicht geaendert haben.
        let current: Value = serde_json::from_slice(&existing).unwrap_or(Value::Null);
        let changed = patch.iter().any(|(k, v)| current.get(k) != Some(v));
        if !changed {
            return Ok(Write::Unchanged);
        }
        if !dry_run {
            let (status, text) = client.request("PATCH", &path, Some(&Value::Object(patch)))?;
            check(status, &text)?;
        }
        return Ok(Write::Updated);
    }
    if status != 404 {
        bail!("GET {path}: HTTP {status}");
    }
    if !dry_run {
        let mut create = body.clone();
        if etype.is_draft() {
            // Der Export enthaelt nur aktive Saetze: direkt aktiv anlegen, kein Entwurf.
            create.insert("IsActiveEntity".into(), Value::Bool(true));
        }
        let (status, text) = client.request("POST", &format!("/{set}"), Some(&Value::Object(create)))?;
        check(status, &text)?;
    }
    Ok(Write::Created)
}

fn check(status: u16, text: &[u8]) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        bail!("HTTP {status}: {}", String::from_utf8_lossy(&text[..text.len().min(400)]))
    }
}

/// Uebersetzt Fremdschluessel, die auf dem Ziel anders heissen als in der Quelle.
fn translate_foreign_keys(
    model: &Model,
    etype: &EntityType,
    record: &Map<String, Value>,
    source_index: &HashMap<String, HashMap<String, Map<String, Value>>>,
    body: &mut Map<String, Value>,
) -> std::result::Result<(), String> {
    for (nav_name, nav) in &etype.navigations {
        if !nav.to_one {
            continue;
        }
        for (prop, referenced) in &nav.refs {
            if body.contains_key(prop) {
                continue;
            }
            let src_fk = format!("{nav_name}_ID");
            let Some(value) = record.get(&src_fk).filter(|v| !v.is_null()) else {
                continue;
            };
            let parent_set = model
                .set_for_type(&nav.target)
                .ok_or_else(|| format!("kein Set fuer {}", nav.target))?;
            let parent = source_index
                .get(parent_set)
                .and_then(|i| i.get(&value_to_key(value)))
                .ok_or_else(|| format!("{src_fk}={} nicht in der Quelle", value_to_key(value)))?;
            let v = parent
                .get(referenced)
                .ok_or_else(|| format!("Zielspalte {referenced} fehlt im Quellsatz"))?;
            body.insert(prop.clone(), v.clone());
        }
    }
    Ok(())
}

fn value_to_key(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn source_sets(manifest: &Value) -> Result<HashMap<String, String>> {
    let mut out = HashMap::new();
    for s in manifest["sets"].as_array().context("manifest.sets fehlt")? {
        let set = s["set"].as_str().context("set fehlt")?;
        let file = s["file"].as_str().context("file fehlt")?;
        out.insert(set.to_string(), file.to_string());
    }
    Ok(out)
}

fn read_records(dir: &Path, file: &str) -> Result<Vec<Value>> {
    match read_json(&dir.join(file))? {
        Value::Array(items) => Ok(items),
        _ => bail!("{file}: Liste erwartet"),
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let text = fs::read_to_string(path).with_context(|| format!("{} nicht lesbar", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("{} kein gueltiges JSON", path.display()))
}
