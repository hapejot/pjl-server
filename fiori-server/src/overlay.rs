//! Overlay zur generierten `ddic.json` (nur Laufzeit).
//!
//! Die Basis-Datei wird bei jeder Migration neu erzeugt. Das Overlay ergaenzt
//! oder ueberschreibt Eigenschaften einzelner Entitaeten, ohne die Basis zu
//! veraendern. Der Merge erfolgt ueber den Entitaetsnamen.
//!
//! Regeln:
//! - `type_name_plural`, `package`: ueberschreiben die Basis.
//! - `list_views`: je Ansicht per `name`. Gleichnamige ersetzen die Basis,
//!   unbekannte werden angehaengt.
//! - `object_page_hidden`: Vereinigung mit der Basis. Ein Overlay kann Felder
//!   ausblenden, aber nicht wieder einblenden.
//! - Unbekannte Entitaeten, Pakete und Felder sind Fehler.
//! - `fields` ist nicht Teil des Overlays (unbekannte Schluessel werden abgelehnt).

use std::path::Path;

use anyhow::{bail, Context};
use serde::Deserialize;
use serde_json::{json, Value};
use spec::{data_dictionary::DataDictionary, ListView};

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
    pub package: Option<String>,
    #[serde(default)]
    pub type_name_plural: Option<String>,
    #[serde(default)]
    pub list_views: Vec<ListView>,
    #[serde(default)]
    pub object_page_hidden: Vec<String>,
}

/// Overlay-Datei lesen und parsen.
pub fn load(path: &Path) -> anyhow::Result<Overlay> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read ddic overlay '{}'", path.display()))?;
    parse(&text).with_context(|| format!("'{}' is not a valid ddic overlay", path.display()))
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
                    .position(|v| v.get("name").and_then(Value::as_str) == Some(view.name.as_str()))
                {
                    Some(i) => views[i] = value,
                    None => views.push(value),
                }
            }
        }

        if !oe.object_page_hidden.is_empty() {
            let hidden = obj
                .as_object_mut()
                .context("ddic: Entitaet ist kein Objekt")?
                .entry("object_page_hidden")
                .or_insert_with(|| json!([]))
                .as_array_mut()
                .context("ddic: 'object_page_hidden' ist kein Array")?;
            for field in &oe.object_page_hidden {
                if !hidden.iter().any(|h| h.as_str() == Some(field.as_str())) {
                    hidden.push(json!(field));
                }
            }
        }
    }

    Ok(serde_json::from_value(value)?)
}

/// Prueft Referenzen gegen das Basis-Dictionary, bevor etwas veraendert wird.
fn validate(base: &DataDictionary, overlay: &Overlay) -> anyhow::Result<()> {
    let entities = base.entities();
    let mut seen: Vec<&str> = Vec::new();
    for oe in &overlay.entities {
        if seen.contains(&oe.name.as_str()) {
            bail!("overlay: Entitaet '{}' mehrfach angegeben", oe.name);
        }
        seen.push(&oe.name);

        let Some(entity) = entities.iter().find(|e| e.name() == oe.name) else {
            bail!("overlay: Entitaet '{}' fehlt in der ddic.json", oe.name);
        };
        if let Some(package) = &oe.package {
            if !base.packages().any(|p| p.name() == package) {
                bail!("overlay: Entitaet '{}': Paket '{package}' unbekannt", oe.name);
            }
        }
        for field in &oe.object_page_hidden {
            if !entity.fields().any(|f| f.name() == field) {
                bail!(
                    "overlay: Entitaet '{}': object_page_hidden nennt unbekanntes Feld '{field}'",
                    oe.name
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use spec::{EntitySpec, FieldSpec, ListView};

    fn base() -> DataDictionary {
        let mut d = DataDictionary::empty();
        d.add_package(spec::package::Package::new("core"));
        d.add_entity(
            EntitySpec::new("Video", "core", "Videos")
                .field(FieldSpec::atom("name", "Name", "Edm.String"))
                .field(FieldSpec::atom("file_size", "Size", "Edm.Int64"))
                .list_view(ListView::new("default", &["name", "file_size"]))
                .list_view(ListView::new("review", &["name"])),
        );
        d
    }

    fn video(d: &DataDictionary) -> EntitySpec {
        d.entities().into_iter().find(|e| e.name() == "Video").unwrap()
    }

    #[test]
    fn list_views_merge_by_name() {
        let overlay = parse(
            r#"{"entities":[{"name":"Video","list_views":[
                {"name":"default","columns":["file_size"]},
                {"name":"archive","columns":["name","file_size"]}]}]}"#,
        )
        .unwrap();
        let d = apply(base(), &overlay).unwrap();
        let views = video(&d);
        let views = views.list_views();
        assert_eq!(views.len(), 3);
        let default = views.iter().find(|v| v.name == "default").unwrap();
        assert_eq!(default.columns, vec!["file_size"]);
        let review = views.iter().find(|v| v.name == "review").unwrap();
        assert_eq!(review.columns, vec!["name"]);
        assert!(views.iter().any(|v| v.name == "archive"));
    }

    #[test]
    fn object_page_hidden_is_a_union() {
        let overlay = parse(
            r#"{"entities":[{"name":"Video","object_page_hidden":["file_size"]}]}"#,
        )
        .unwrap();
        let once = apply(base(), &overlay).unwrap();
        let twice = apply(once, &overlay).unwrap();
        assert_eq!(video(&twice).object_page_hidden(), &["file_size".to_string()]);
    }

    #[test]
    fn scalars_override_the_base() {
        let overlay =
            parse(r#"{"entities":[{"name":"Video","type_name_plural":"Filme"}]}"#).unwrap();
        let d = apply(base(), &overlay).unwrap();
        assert_eq!(video(&d).type_name_plural, "Filme");
    }

    #[test]
    fn unknown_entity_is_an_error() {
        let overlay = parse(r#"{"entities":[{"name":"Nope"}]}"#).unwrap();
        let err = apply(base(), &overlay).unwrap_err().to_string();
        assert!(err.contains("Nope"), "{err}");
    }

    #[test]
    fn unknown_hidden_field_is_an_error() {
        let overlay =
            parse(r#"{"entities":[{"name":"Video","object_page_hidden":["gibts_nicht"]}]}"#)
                .unwrap();
        assert!(apply(base(), &overlay).is_err());
    }

    #[test]
    fn unknown_keys_are_rejected() {
        // Tippfehler duerfen nicht still verschluckt werden.
        assert!(parse(r#"{"entities":[{"name":"Video","list_view":[]}]}"#).is_err());
        assert!(parse(r#"{"entities":[{"name":"Video","fields":[]}]}"#).is_err());
    }

    #[test]
    fn empty_overlay_keeps_the_base() {
        let d = apply(base(), &parse("{}").unwrap()).unwrap();
        assert_eq!(video(&d).list_views().len(), 2);
    }
}
