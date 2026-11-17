//! Zusammensetzen und Zerlegen paketierter Dictionaries.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use file_repo::FileRepository;
use serde_json::{Map, Value, json};

const GROUPS: [&str; 4] = ["entities", "relationships", "value_lists", "components"];

#[derive(Clone, Debug)]
pub struct Layout {
    pub imports: Vec<String>,
    pub files: BTreeMap<String, Value>,
    pub root: Value,
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value]> {
    match value.get(key) {
        None => Ok(&[]),
        Some(Value::Array(values)) => Ok(values),
        _ => bail!("'{key}' muss eine Liste sein"),
    }
}

fn package_path(base: &str, name: &str) -> Result<String> {
    ensure!(
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'),
        "ungueltiger Paketname '{name}'"
    );
    let parent = Path::new(base).parent().unwrap_or_else(|| Path::new(""));
    Ok(parent
        .join("packages")
        .join(format!("{name}.json"))
        .to_string_lossy()
        .replace('\\', "/"))
}

pub fn load(repo: &dyn FileRepository, path: &str) -> Result<(Value, Option<Layout>)> {
    let bytes = repo
        .read(path)
        .with_context(|| format!("cannot read '{path}'"))?;
    let mut root: Value =
        serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in '{path}'"))?;
    ensure!(root.is_object(), "'{path}': Objekt erwartet");
    let Some(imports) = root.get("package_files") else {
        return Ok((root, None));
    };
    let original_root = root.clone();
    let imports = imports
        .as_array()
        .context("package_files muss eine Liste sein")?
        .clone();
    let mut names = HashSet::new();
    let mut files = BTreeMap::new();
    let mut ordered = Vec::new();
    for entry in &imports {
        let name = entry
            .as_str()
            .context("package_files erwartet Paketnamen als Text")?;
        ensure!(
            names.insert(name.to_string()),
            "Paket '{name}' mehrfach referenziert"
        );
        let file_path = package_path(path, name)?;
        let bytes = repo
            .read(&file_path)
            .with_context(|| format!("cannot read package '{file_path}'"))?;
        let file: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("invalid JSON in '{file_path}'"))?;
        ensure!(file.is_object(), "'{file_path}': Objekt erwartet");
        ensure!(
            file.get("package_files").is_none(),
            "'{file_path}': verschachtelte Paketreferenzen werden nicht unterstuetzt"
        );
        ensure!(
            array(&file, "apps")?.is_empty() && array(&file, "packages")?.is_empty(),
            "'{file_path}': apps und packages gehoeren in das Hauptdictionary"
        );
        for group in GROUPS {
            let mut rows = array(&file, group)?.to_vec();
            for row in &mut rows {
                let obj = row
                    .as_object_mut()
                    .with_context(|| format!("'{file_path}': {group} erwartet Objekte"))?;
                match obj.get("package") {
                    Some(Value::String(existing)) if existing == name => {}
                    None => {
                        obj.insert("package".into(), json!(name));
                    }
                    _ => {
                        bail!("'{file_path}': {group} enthaelt ein Objekt aus einem anderen Paket")
                    }
                }
            }
            root.as_object_mut()
                .unwrap()
                .entry(group)
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .with_context(|| format!("'{group}' muss eine Liste sein"))?
                .extend(rows);
        }
        let uses = array(&file, "component_uses")?.to_vec();
        root.as_object_mut()
            .unwrap()
            .entry("component_uses")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context("component_uses muss eine Liste sein")?
            .extend(uses);
        files.insert(name.to_string(), file);
        ordered.push(name.to_string());
    }
    validate(&root)?;
    Ok((
        root,
        Some(Layout {
            imports: ordered,
            files,
            root: original_root,
        }),
    ))
}

fn validate(root: &Value) -> Result<()> {
    for (group, key) in [
        ("packages", "name"),
        ("entities", "name"),
        ("relationships", "name"),
        ("apps", "name"),
        ("components", "name"),
        ("value_lists", "name"),
        ("value_lists", "id"),
    ] {
        let mut seen = HashSet::new();
        for row in array(root, group)? {
            let value = row
                .get(key)
                .and_then(Value::as_str)
                .with_context(|| format!("{group}: {key} fehlt"))?;
            let identity = if group == "value_lists" && key == "id" {
                uuid::Uuid::parse_str(value)
                    .with_context(|| format!("{group}: ungueltige ID '{value}'"))?
                    .to_string()
            } else {
                value.to_string()
            };
            ensure!(
                seen.insert(identity),
                "{group}: {key} '{value}' mehrfach definiert"
            );
        }
    }
    let packages: HashSet<_> = array(root, "packages")?
        .iter()
        .filter_map(|p| p.get("name").and_then(Value::as_str))
        .collect();
    for name in array(root, "package_files")? {
        let name = name.as_str().context("ungueltiger Paketname")?;
        ensure!(
            packages.contains(name),
            "Paket '{name}' ist nicht in packages definiert"
        );
    }
    for group in GROUPS {
        for object in array(root, group)? {
            if let Some(package) = object.get("package") {
                let package = package
                    .as_str()
                    .with_context(|| format!("{group}: package muss Text sein"))?;
                ensure!(
                    packages.contains(package),
                    "{group}: unbekanntes Paket '{package}'"
                );
            }
        }
    }
    let mut uses = HashSet::new();
    for row in array(root, "component_uses")? {
        let entity = row
            .get("entity")
            .and_then(Value::as_str)
            .context("component_use: entity fehlt")?;
        let instance = row
            .get("instance")
            .and_then(Value::as_str)
            .context("component_use: instance fehlt")?;
        ensure!(
            uses.insert((entity, instance)),
            "component_use '{entity}/{instance}' mehrfach definiert"
        );
    }
    Ok(())
}

pub fn split(base: &str, combined: &Value, layout: &Layout) -> Result<BTreeMap<String, Value>> {
    validate(combined)?;
    let mut root = combined
        .as_object()
        .cloned()
        .context("Dictionary muss ein Objekt sein")?;
    let mut packages: BTreeMap<String, Map<String, Value>> = layout
        .imports
        .iter()
        .map(|name| {
            let mut obj = layout.files[name].as_object().cloned().unwrap_or_default();
            for key in GROUPS.iter().chain(["component_uses"].iter()) {
                obj.remove(*key);
            }
            (name.clone(), obj)
        })
        .collect();
    let mut entity_packages = HashMap::new();
    for entity in array(combined, "entities")? {
        let name = entity
            .get("name")
            .and_then(Value::as_str)
            .context("entity name fehlt")?;
        if let Some(package) = entity.get("package").and_then(Value::as_str) {
            entity_packages.insert(name.to_string(), package.to_string());
        }
    }
    for group in GROUPS {
        let mut local = Vec::new();
        for row in array(combined, group)? {
            match row
                .get("package")
                .and_then(Value::as_str)
                .and_then(|name| packages.get_mut(name))
            {
                Some(target) => target
                    .entry(group)
                    .or_insert_with(|| json!([]))
                    .as_array_mut()
                    .unwrap()
                    .push(row.clone()),
                None => local.push(row.clone()),
            }
        }
        root.insert(group.into(), Value::Array(local));
    }
    let mut local = Vec::new();
    for row in array(combined, "component_uses")? {
        let entity = row
            .get("entity")
            .and_then(Value::as_str)
            .context("component_use: entity fehlt")?;
        match entity_packages
            .get(entity)
            .and_then(|name| packages.get_mut(name))
        {
            Some(target) => target
                .entry("component_uses")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .unwrap()
                .push(row.clone()),
            None => local.push(row.clone()),
        }
    }
    root.insert("component_uses".into(), Value::Array(local));
    let mut output = BTreeMap::new();
    output.insert(base.to_string(), Value::Object(root));
    for (name, value) in packages {
        output.insert(package_path(base, &name)?, Value::Object(value));
    }
    Ok(output)
}
