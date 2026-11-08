//! Exportiert die aktiven Daten eines OData-Services in ein Verzeichnis.
//!
//! Dasselbe Format wie `scripts/odata-export.py`, damit `scripts/odata-import.py`
//! den Stand einspielen kann. Das Programm ist ein statisches Standalone-Binary
//! ohne Laufzeitabhaengigkeiten, es laeuft also auch in Containern ohne Python.
//!
//!     odata-export --base-url http://localhost:8080/Service --out /tmp/export

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use serde_json::{Map, Value, json};

use odata_export::http::Client;
use odata_export::model::{EntityType, Model};
use odata_export::{DRAFT_PROPERTIES, FORMAT_VERSION};

const PAGE: usize = 500;

#[derive(Parser, Debug)]
#[command(about = "Exportiert die aktiven Daten eines OData-Services in ein Verzeichnis")]
struct Args {
    /// Service-Wurzel, z. B. http://localhost:8080/Service (nur http://)
    #[arg(long)]
    base_url: String,
    /// Zielverzeichnis (wird angelegt, vorhandene Dateien werden ueberschrieben)
    #[arg(long)]
    out: PathBuf,
    /// Komma-getrennte Setnamen (Standard: alle)
    #[arg(long)]
    sets: Option<String>,
    /// Benutzer, unter dem der Export laeuft (Header siehe --user-header)
    #[arg(long, default_value = "export@local")]
    user: String,
    /// Header, in dem der Auth-Proxy den Benutzer erwartet (Server: AUTH_USER_HEADER)
    #[arg(long, default_value = "X-Forwarded-User")]
    user_header: String,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Fehler: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let client = Client::new(&args.base_url)?.with_user(&args.user_header, &args.user);

    let (status, xml) = client.get("/$metadata")?;
    if status != 200 {
        bail!("$metadata lieferte HTTP {status}");
    }
    let xml = String::from_utf8(xml).context("$metadata ist kein UTF-8")?;
    let model = Model::parse(&xml)?;

    let wanted: Vec<String> = match &args.sets {
        Some(list) => list.split(',').map(|s| s.trim().to_string()).collect(),
        None => model.sets.keys().cloned().collect(),
    };
    for set in &wanted {
        if !model.sets.contains_key(set) {
            bail!("unbekanntes Set: {set}");
        }
    }
    let order = model.order(&wanted)?;

    fs::create_dir_all(&args.out)
        .with_context(|| format!("Verzeichnis {} nicht anlegbar", args.out.display()))?;
    fs::write(args.out.join("metadata.xml"), &xml)?;

    let mut manifest_sets = Vec::new();
    let mut total = 0usize;
    for set in &order {
        let etype = model.set_type(set).context("Typ fehlt")?;
        let raw = fetch_all(&client, set)?;
        let keep: Vec<&String> = etype
            .properties
            .iter()
            .filter(|p| !DRAFT_PROPERTIES.contains(&p.as_str()))
            .collect();
        let records: Vec<Value> = raw
            .iter()
            .filter(|r| r.get("IsActiveEntity") != Some(&Value::Bool(false)))
            .map(|r| {
                let mut out = Map::new();
                for p in &keep {
                    if let Some(v) = r.get(p.as_str()) {
                        out.insert(p.to_string(), v.clone());
                    }
                }
                Value::Object(out)
            })
            .collect();
        let file = format!("{set}.json");
        fs::write(
            args.out.join(&file),
            serde_json::to_string_pretty(&records)?,
        )?;
        println!("{:32} {:6} Saetze", set, records.len());
        total += records.len();
        manifest_sets.push(manifest_entry(&model, set, etype, records.len(), &file));
    }

    let manifest = json!({
        "format": FORMAT_VERSION,
        "source": args.base_url,
        "exported_at_unix": now_unix(),
        "order": order,
        "sets": manifest_sets,
    });
    fs::write(
        args.out.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;
    println!(
        "{} Sets, {total} Saetze nach {}",
        order.len(),
        args.out.display()
    );
    Ok(())
}

fn fetch_all(client: &Client, set: &str) -> Result<Vec<Value>> {
    let mut records = Vec::new();
    let mut skip = 0usize;
    loop {
        let (status, body) = client.get(&format!("/{set}?$top={PAGE}&$skip={skip}"))?;
        if status != 200 {
            bail!("{set}: HTTP {status}: {}", String::from_utf8_lossy(&body));
        }
        let page: Value =
            serde_json::from_slice(&body).with_context(|| format!("{set}: ungueltiges JSON"))?;
        let items = page
            .get("value")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let n = items.len();
        records.extend(items);
        if n < PAGE {
            return Ok(records);
        }
        skip += PAGE;
    }
}

fn manifest_entry(model: &Model, set: &str, etype: &EntityType, count: usize, file: &str) -> Value {
    let navigations: Map<String, Value> = etype
        .navigations
        .iter()
        .map(|(name, nav)| {
            let refs: Vec<Value> = nav.refs.iter().map(|(p, r)| json!([p, r])).collect();
            (
                name.clone(),
                json!({
                    "target_set": model.set_for_type(&nav.target),
                    "to_one": nav.to_one,
                    "refs": refs,
                }),
            )
        })
        .collect();
    let keys: BTreeSet<&String> = etype.keys.iter().collect();
    json!({
        "set": set,
        "type": etype.name,
        "keys": keys.into_iter().cloned().collect::<Vec<_>>(),
        "draft": etype.is_draft(),
        "count": count,
        "file": file,
        "navigations": navigations,
    })
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
