//! Experimentelles Bearbeiten des Data Dictionary aus der 3270-Schnittstelle.
//!
//! Die Aenderungen liegen nur im Speicher, als JSON-Entwurf (eine Kopie der
//! Basis). Sie wirken nicht auf den Store und nicht auf die `ddic.json`.
//! `reload` validiert den Entwurf und macht ihn zur neuen Basis; `dump ddic`
//! schreibt den Entwurf in das Dump-Verzeichnis.

use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};
use spec::data_dictionary::DataDictionary;

use crate::AppState;

/// Ergebnis eines Befehls fuer die Anzeige.
pub enum Reply {
    /// Meldung in der Statuszeile.
    Msg(String),
    /// Zeilen als Ansicht (Bezeichnung, Wert).
    Show {
        title: String,
        rows: Vec<(String, String)>,
    },
}

/// Befehlsworte, die der Editor selbst auswertet.
const COMMANDS: &[&str] = &["help", "dump", "apps", "app", "entities", "entity", "reload"];

pub struct DdicEditor {
    state: AppState,
    dump_dir: PathBuf,
    draft: Mutex<Value>,
}

impl DdicEditor {
    /// Entwurf startet als Kopie der aktuellen Basis.
    pub fn new(state: AppState, dump_dir: PathBuf) -> Self {
        let draft = serde_json::to_value(&*state.base()).unwrap_or(Value::Null);
        Self {
            state,
            dump_dir,
            draft: Mutex::new(draft),
        }
    }

    /// `None`, wenn die Zeile mit keinem Editor-Befehl beginnt.
    pub fn run(&self, line: &str) -> Option<Reply> {
        let words: Vec<&str> = line.split_whitespace().collect();
        let first = words.first()?.to_lowercase();
        if !COMMANDS.contains(&first.as_str()) {
            return None;
        }
        let mut draft = self.draft.lock().unwrap_or_else(|p| p.into_inner());
        Some(match self.exec(&mut draft, &first, &words[1..]) {
            Ok(reply) => reply,
            Err(e) => Reply::Msg(e),
        })
    }

    fn exec(&self, draft: &mut Value, cmd: &str, args: &[&str]) -> Result<Reply, String> {
        match cmd {
            "help" => Ok(show("Befehle", HELP.iter().map(|l| (String::new(), l.to_string())).collect())),
            "dump" => match args {
                ["ddic"] => self.dump(draft),
                _ => Err("Verwendung: dump ddic".into()),
            },
            "apps" => Ok(show("Apps", apps_rows(draft))),
            "app" => app_cmd(draft, args),
            "entities" => Ok(show("Entities", entity_rows(draft))),
            "entity" => entity_cmd(draft, args),
            "reload" => self.reload(draft),
            _ => Err(format!("Unbekannter Befehl: {cmd}")),
        }
    }

    fn dump(&self, draft: &Value) -> Result<Reply, String> {
        std::fs::create_dir_all(&self.dump_dir).map_err(|e| format!("Dump fehlgeschlagen: {e}"))?;
        let dst = self.dump_dir.join("ddic.json");
        let text = serde_json::to_string_pretty(draft).map_err(|e| e.to_string())?;
        std::fs::write(&dst, text).map_err(|e| format!("Dump fehlgeschlagen: {e}"))?;
        tracing::info!(dst = %dst.display(), "ddic draft dumped");
        Ok(Reply::Msg(format!("Entwurf geschrieben nach {}", dst.display())))
    }

    fn reload(&self, draft: &Value) -> Result<Reply, String> {
        let ddic: DataDictionary = serde_json::from_value(draft.clone())
            .map_err(|e| format!("Entwurf ungueltig: {e}"))?;
        ddic.validate_list_views()
            .map_err(|e| format!("Listenansicht ungueltig: {e}"))?;
        let counts = format!(
            "{} Entities, {} Apps",
            ddic.entities().len(),
            ddic.apps().count()
        );
        self.state.replace_base(ddic);
        tracing::warn!("ddic draft applied in memory via 3270");
        Ok(Reply::Msg(format!("Entwurf aktiv (nur Speicher): {counts}")))
    }
}

const HELP: &[&str] = &[
    "apps                          Apps auflisten",
    "app <name>                    App anzeigen",
    "app <name> title|subtitle|icon <text>",
    "                              Feld aendern (- leert subtitle/icon)",
    "app new <name> <entity> <titel>   App anlegen",
    "entities                      Entities auflisten",
    "entity <name>                 Entity mit Feldern anzeigen",
    "entity <name> plural <text>   Plural-Bezeichnung aendern",
    "reload                        Entwurf aktivieren (nur Speicher)",
    "dump ddic                     Entwurf nach DUMP_DIR schreiben",
];

fn show(title: &str, rows: Vec<(String, String)>) -> Reply {
    Reply::Show {
        title: title.to_string(),
        rows,
    }
}

fn str_of(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => "-".into(),
        Some(other) => other.to_string(),
    }
}

fn items<'a>(draft: &'a Value, key: &str) -> &'a [Value] {
    draft.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn items_mut<'a>(draft: &'a mut Value, key: &str) -> Result<&'a mut Vec<Value>, String> {
    draft
        .get_mut(key)
        .and_then(Value::as_array_mut)
        .ok_or_else(|| format!("Entwurf ohne Liste '{key}'"))
}

fn named<'a>(list: &'a [Value], name: &str) -> Option<&'a Value> {
    list.iter().find(|v| v.get("name").and_then(Value::as_str) == Some(name))
}

fn named_mut<'a>(list: &'a mut [Value], name: &str) -> Option<&'a mut Value> {
    list.iter_mut()
        .find(|v| v.get("name").and_then(Value::as_str) == Some(name))
}

fn apps_rows(draft: &Value) -> Vec<(String, String)> {
    items(draft, "apps")
        .iter()
        .map(|a| {
            (
                str_of(a, "name"),
                format!(
                    "{} | {} | {}",
                    str_of(a, "title"),
                    str_of(a, "main_entity"),
                    a.get("package").map(|p| str_of(p, "name")).unwrap_or_default()
                ),
            )
        })
        .collect()
}

fn entity_rows(draft: &Value) -> Vec<(String, String)> {
    items(draft, "entities")
        .iter()
        .map(|e| {
            let fields = e.get("fields").and_then(Value::as_array).map_or(0, Vec::len);
            (
                str_of(e, "name"),
                format!("{} | {} | {fields} Felder", str_of(e, "type_name_plural"), str_of(e, "package")),
            )
        })
        .collect()
}

fn app_cmd(draft: &mut Value, args: &[&str]) -> Result<Reply, String> {
    match args {
        [] => Err("Verwendung: app <name> | app new <name> <entity> <titel> | app <name> title <text>".into()),
        ["new", name, entity, title @ ..] => {
            if title.is_empty() {
                return Err("Titel fehlt".into());
            }
            if named(items(draft, "apps"), name).is_some() {
                return Err(format!("App '{name}' existiert bereits"));
            }
            let ent = named(items(draft, "entities"), entity)
                .ok_or_else(|| format!("Entity '{entity}' unbekannt"))?;
            let pkg = str_of(ent, "package");
            let packages = items_mut(draft, "packages")?;
            if named(packages, &pkg).is_none() {
                packages.push(json!({ "name": pkg }));
            }
            items_mut(draft, "apps")?.push(json!({
                "name": name,
                "title": title.join(" "),
                "main_entity": entity,
                "subtitle": null,
                "icon": null,
                "package": { "name": pkg },
            }));
            Ok(Reply::Msg(format!("App '{name}' angelegt (Entwurf, mit 'reload' aktivieren)")))
        }
        [name] => {
            let app = named(items(draft, "apps"), name)
                .ok_or_else(|| format!("App '{name}' unbekannt"))?;
            Ok(show(
                &format!("App {name}"),
                vec![
                    ("name".into(), str_of(app, "name")),
                    ("title".into(), str_of(app, "title")),
                    ("main_entity".into(), str_of(app, "main_entity")),
                    ("subtitle".into(), str_of(app, "subtitle")),
                    ("icon".into(), str_of(app, "icon")),
                    (
                        "package".into(),
                        app.get("package").map(|p| str_of(p, "name")).unwrap_or_default(),
                    ),
                ],
            ))
        }
        [name, field @ ("title" | "subtitle" | "icon"), value @ ..] => {
            if value.is_empty() {
                return Err(format!("Wert fehlt fuer {field}"));
            }
            let text = value.join(" ");
            let new = match (*field, text.as_str()) {
                ("title", "-") => return Err("Titel darf nicht leer sein".into()),
                (_, "-") => Value::Null,
                _ => Value::String(text),
            };
            let app = named_mut(items_mut(draft, "apps")?, name)
                .ok_or_else(|| format!("App '{name}' unbekannt"))?;
            app[*field] = new;
            Ok(Reply::Msg(format!("App '{name}': {field} geaendert (Entwurf)")))
        }
        _ => Err("Unbekannte app-Anweisung, siehe help".into()),
    }
}

fn entity_cmd(draft: &mut Value, args: &[&str]) -> Result<Reply, String> {
    match args {
        [name] => {
            let ent = named(items(draft, "entities"), name)
                .ok_or_else(|| format!("Entity '{name}' unbekannt"))?;
            let mut rows = vec![
                ("name".to_string(), str_of(ent, "name")),
                ("plural".to_string(), str_of(ent, "type_name_plural")),
                ("package".to_string(), str_of(ent, "package")),
            ];
            for field in ent.get("fields").and_then(Value::as_array).into_iter().flatten() {
                // Felder sind getaggt, z. B. {"Atom": {...}} oder {"Lookup": {...}}.
                if let Some((kind, inner)) = field.as_object().and_then(|o| o.iter().next()) {
                    rows.push((
                        format!("  {}", str_of(inner, "name")),
                        format!("{kind} {}", str_of(inner, "label")),
                    ));
                }
            }
            Ok(show(&format!("Entity {name}"), rows))
        }
        [name, "plural", value @ ..] if !value.is_empty() => {
            let ent = named_mut(items_mut(draft, "entities")?, name)
                .ok_or_else(|| format!("Entity '{name}' unbekannt"))?;
            ent["type_name_plural"] = Value::String(value.join(" "));
            Ok(Reply::Msg(format!("Entity '{name}': plural geaendert (Entwurf)")))
        }
        _ => Err("Verwendung: entity <name> | entity <name> plural <text>".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Value {
        json!({
            "packages": [{ "name": "core" }],
            "entities": [{ "name": "Order", "package": "core", "type_name_plural": "Orders", "fields": [] }],
            "relationships": [],
            "apps": [{ "name": "orders", "title": "Orders", "main_entity": "Order",
                       "subtitle": null, "icon": null, "package": { "name": "core" } }],
        })
    }

    #[test]
    fn app_title_and_new_app_change_only_the_draft() {
        let mut d = draft();
        app_cmd(&mut d, &["orders", "title", "Bestellungen", "neu"]).unwrap();
        assert_eq!(d["apps"][0]["title"], "Bestellungen neu");

        app_cmd(&mut d, &["new", "exp", "Order", "Experiment"]).unwrap();
        assert_eq!(d["apps"][1]["name"], "exp");
        assert_eq!(d["apps"][1]["package"]["name"], "core");
        assert!(app_cmd(&mut d, &["new", "exp", "Order", "x"]).is_err());
        assert!(app_cmd(&mut d, &["new", "x", "Unknown", "x"]).is_err());
    }

    #[test]
    fn dash_clears_optional_text_and_title_stays() {
        let mut d = draft();
        app_cmd(&mut d, &["orders", "subtitle", "Alle"]).unwrap();
        assert_eq!(d["apps"][0]["subtitle"], "Alle");
        app_cmd(&mut d, &["orders", "subtitle", "-"]).unwrap();
        assert!(d["apps"][0]["subtitle"].is_null());
        assert!(app_cmd(&mut d, &["orders", "title", "-"]).is_err());
    }

    #[test]
    fn entity_plural_is_editable() {
        let mut d = draft();
        entity_cmd(&mut d, &["Order", "plural", "Auftraege"]).unwrap();
        assert_eq!(d["entities"][0]["type_name_plural"], "Auftraege");
        assert!(entity_cmd(&mut d, &["Missing", "plural", "x"]).is_err());
    }

    #[test]
    fn draft_roundtrips_into_dictionary() {
        let d: DataDictionary = serde_json::from_value(draft()).unwrap();
        assert_eq!(d.apps().count(), 1);
    }
}
