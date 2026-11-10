//! Overlay zur generierten `ddic.json` (nur Laufzeit).
//!
//! Die Basis-Datei wird bei jeder Migration neu erzeugt. Das Overlay ergaenzt
//! oder ueberschreibt Eigenschaften einzelner Entitaeten, ohne die Basis zu
//! veraendern. Der Merge erfolgt ueber den Entitaetsnamen.
//!
//! Regeln:
//! - `draft_enabled`: ueberschreibt den Draft-Modus der Entity.
//! - `type_name_plural`, `package`: ueberschreiben die Basis.
//! - `list_views`: je Ansicht per `name`. Gleichnamige ersetzen die Basis,
//!   unbekannte werden angehaengt.
//! - `facets`: Object-Page-Abschnitte je Schluessel (siehe `FacetSpec`).
//! - Unbekannte Entitaeten, Pakete und Felder sind Fehler.
//! - `fields` ist nicht Teil des Overlays (unbekannte Schluessel werden abgelehnt).

use std::collections::BTreeMap;

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use spec::{FacetSpec, FilterDefault, ListView, data_dictionary::DataDictionary};

/// Schluessel der Allgemein-Facette und der Sammel-Facette (siehe `srv::GENERAL_FACET`, `srv::DEFAULT_FACET`).
const GENERAL_FACET: &str = "General";
const DEFAULT_FACET: &str = "Default";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    #[serde(default)]
    pub entities: Vec<OverlayEntity>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayEntity {
    pub name: String,
    #[serde(default)]
    pub draft_enabled: Option<bool>,
    #[serde(default)]
    pub package: Option<String>,
    #[serde(default)]
    pub type_name: Option<String>,
    #[serde(default)]
    pub type_name_plural: Option<String>,
    #[serde(default)]
    pub list_views: Vec<ListView>,
    #[serde(default)]
    pub filter_defaults: BTreeMap<String, FilterDefault>,
    #[serde(default)]
    pub selection_fields: Vec<String>,
    #[serde(default)]
    pub facets: Vec<FacetSpec>,
}

/// Overlay-Datei aus der Quelle lesen und parsen.
pub fn load(file: &crate::sources::FileRef) -> anyhow::Result<Overlay> {
    let bytes = file
        .repo
        .read(&file.rel)
        .with_context(|| format!("cannot read ddic overlay '{}'", file.label))?;
    let text =
        std::str::from_utf8(&bytes).with_context(|| format!("'{}' is not UTF-8", file.label))?;
    parse(text).with_context(|| format!("'{}' is not a valid ddic overlay", file.label))
}

pub fn parse(text: &str) -> anyhow::Result<Overlay> {
    Ok(serde_json::from_str(text)?)
}

/// Overlay auf das Basis-Dictionary anwenden. Liefert ein neues Dictionary.
pub fn apply(base: DataDictionary, overlay: &Overlay) -> anyhow::Result<DataDictionary> {
    validate(&base, overlay)?;
    if overlay.entities.is_empty() {
        return Ok(base);
    }

    let mut value = serde_json::to_value(&base)?;
    let entities = value
        .get_mut("entities")
        .and_then(Value::as_array_mut)
        .context("ddic: 'entities' fehlt")?;

    for oe in &overlay.entities {
        let obj = entities
            .iter_mut()
            .find(|e| e.get("name").and_then(Value::as_str) == Some(oe.name.as_str()))
            .with_context(|| format!("overlay: Entitaet '{}' fehlt in der ddic.json", oe.name))?;

        if let Some(package) = &oe.package {
            obj["package"] = json!(package);
        }
        if let Some(enabled) = oe.draft_enabled {
            obj["draft_enabled"] = json!(enabled);
        }
        if let Some(name) = &oe.type_name {
            obj["type_name"] = json!(name);
        }
        if let Some(plural) = &oe.type_name_plural {
            obj["type_name_plural"] = json!(plural);
        }

        if !oe.list_views.is_empty() {
            let views = obj
                .as_object_mut()
                .context("ddic: Entitaet ist kein Objekt")?
                .entry("list_views")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("ddic: 'list_views' ist kein Array")?;
            for view in &oe.list_views {
                let value = serde_json::to_value(view)?;
                match views
                    .iter()
                    .position(|v| v.get("name").and_then(Value::as_str) == Some(view.name()))
                {
                    Some(i) => views[i] = value,
                    None => views.push(value),
                }
            }
        }

        if !oe.selection_fields.is_empty() {
            let fields = obj
                .as_object_mut()
                .context("ddic: Entitaet ist kein Objekt")?
                .entry("selection_fields")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("ddic: 'selection_fields' ist kein Array")?;
            for field in &oe.selection_fields {
                if !fields.iter().any(|f| f.as_str() == Some(field.as_str())) {
                    fields.push(json!(field));
                }
            }
        }

        if !oe.facets.is_empty() {
            let facets = obj
                .as_object_mut()
                .context("ddic: Entitaet ist kein Objekt")?
                .entry("facets")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("ddic: 'facets' ist kein Array")?;
            for facet in &oe.facets {
                let value = serde_json::to_value(facet)?;
                match facets
                    .iter()
                    .position(|f| f.get("facet").and_then(Value::as_str) == Some(facet.facet()))
                {
                    Some(i) => facets[i] = value,
                    None => facets.push(value),
                }
            }
        }

        if !oe.filter_defaults.is_empty() {
            let defaults = obj
                .as_object_mut()
                .context("ddic: Entitaet ist kein Objekt")?
                .entry("filter_defaults")
                .or_insert_with(|| json!({}))
                .as_object_mut()
                .context("ddic: 'filter_defaults' ist kein Objekt")?;
            for (field, value) in &oe.filter_defaults {
                defaults.insert(field.clone(), serde_json::to_value(value)?);
            }
        }
    }

    let merged: DataDictionary = serde_json::from_value(value)?;
    validate_facets(&merged, overlay)?;
    Ok(merged)
}

/// Prueft Referenzen gegen das Basis-Dictionary, bevor etwas veraendert wird.
fn validate(base: &DataDictionary, overlay: &Overlay) -> anyhow::Result<()> {
    // Aktivierter Stand: einwertige Navigationen (Lookups) entstehen erst aus den Relationships.
    let mut active = base.clone();
    active.activate();
    let mut seen: Vec<&str> = Vec::new();
    for oe in &overlay.entities {
        if seen.contains(&oe.name.as_str()) {
            bail!("overlay: Entitaet '{}' mehrfach angegeben", oe.name);
        }
        seen.push(&oe.name);

        let Some(entity) = active.active_entity(&oe.name) else {
            bail!("overlay: Entitaet '{}' fehlt in der ddic.json", oe.name);
        };
        if let Some(package) = &oe.package {
            if !base.packages().any(|p| p.name() == package) {
                bail!(
                    "overlay: Entitaet '{}': Paket '{package}' unbekannt",
                    oe.name
                );
            }
        }
        for field in &oe.selection_fields {
            let ok = entity.fields().any(|f| match f {
                spec::FieldSpec::Atom { name, .. } => name == field,
                spec::FieldSpec::Lookup {
                    name, collection, ..
                } => name == field && !collection,
                spec::FieldSpec::Measure { .. } => false,
            });
            if !ok {
                bail!(
                    "overlay: Entitaet '{}': selection_fields nennt '{field}', das weder Atom noch einwertige Navigation ist",
                    oe.name
                );
            }
        }
        for field in oe.filter_defaults.keys() {
            let is_atom = entity
                .fields()
                .any(|f| f.name() == field && matches!(f, spec::FieldSpec::Atom { .. }));
            if !is_atom {
                bail!(
                    "overlay: Entitaet '{}': filter_defaults nennt '{field}', das kein Atom-Feld ist",
                    oe.name
                );
            }
        }
    }
    Ok(())
}

/// Prueft `facets` gegen das gemergte Dictionary.
/// * `Default`: ohne `fields` und ohne `view`.
/// * `General`: nur `title` und `fields`.
/// * To-many Navigation: `view` und `title`, keine `fields`.
/// * Eigener Schluessel: braucht `fields`, keine `view`.
fn validate_facets(merged: &DataDictionary, overlay: &Overlay) -> anyhow::Result<()> {
    let mut active = merged.clone();
    active.activate();
    for oe in &overlay.entities {
        let entity = active
            .active_entity(&oe.name)
            .with_context(|| format!("overlay: Entitaet '{}' fehlt", oe.name))?;
        let mut seen: Vec<&str> = Vec::new();
        for facet in &oe.facets {
            let key = facet.facet();
            let ctx = format!("overlay: Entitaet '{}': facet '{key}'", oe.name);
            if facet
                .title()
                .as_deref()
                .is_some_and(|t| t.trim().is_empty())
            {
                bail!("{ctx} hat einen leeren title");
            }
            if seen.contains(&key) {
                bail!("{ctx} mehrfach angegeben");
            }
            seen.push(key);
            let mut seen_fields: Vec<&str> = Vec::new();
            for field in facet.fields() {
                if seen_fields.contains(&field.as_str()) {
                    bail!("{ctx}: Feld '{field}' mehrfach angegeben");
                }
                seen_fields.push(field);
            }

            let nav_target = entity.fields().find_map(|f| match f {
                spec::FieldSpec::Lookup {
                    name,
                    entity: target,
                    collection: true,
                    ..
                } if name == key => Some(target.clone()),
                _ => None,
            });

            if key == DEFAULT_FACET {
                if facet.view().is_some() || !facet.fields().is_empty() {
                    bail!(
                        "{ctx}: Default hat keine fields und keine view, es sammelt die nicht genannten Felder"
                    );
                }
            } else if key == GENERAL_FACET {
                if facet.view().is_some() {
                    bail!("{ctx}: hat keine Listenansicht");
                }
                check_facet_fields(&oe.name, facet, entity)?;
            } else if let Some(target) = nav_target {
                if !facet.fields().is_empty() {
                    bail!("{ctx}: Navigationsfacette hat keine fields");
                }
                if let Some(view) = facet.view().as_deref().filter(|v| *v != ListView::DEFAULT) {
                    let target_entity = active
                        .active_entity(&target)
                        .with_context(|| format!("overlay: Zielentitaet '{target}' fehlt"))?;
                    if !target_entity.list_views().iter().any(|v| v.name() == view) {
                        bail!("{ctx} nennt Ansicht '{view}', die es auf '{target}' nicht gibt");
                    }
                }
            } else {
                if facet.view().is_some() {
                    bail!("{ctx}: ist keine Navigation und hat daher keine Listenansicht");
                }
                if facet.fields().is_empty() {
                    bail!(
                        "{ctx}: ist weder Navigation noch General/Default, eine eigene Facette braucht fields"
                    );
                }
                check_facet_fields(&oe.name, facet, entity)?;
            }
        }
    }
    Ok(())
}

/// Felder einer Facette: Atome oder einwertige Navigationen.
fn check_facet_fields(
    owner: &str,
    facet: &FacetSpec,
    entity: &spec::EntitySpec,
) -> anyhow::Result<()> {
    for field in facet.fields() {
        let ok = entity.fields().any(|f| match f {
            spec::FieldSpec::Atom { name, .. } => name == field,
            spec::FieldSpec::Lookup {
                name, collection, ..
            } => name == field && !collection,
            spec::FieldSpec::Measure { .. } => false,
        });
        if !ok {
            bail!(
                "overlay: Entitaet '{owner}': facet '{}' nennt '{field}', das weder Atom noch einwertige Navigation ist",
                facet.facet()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
