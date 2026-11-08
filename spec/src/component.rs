//! Wiederverwendbare Komponenten (z. B. ein Zeitraum aus Beginn, Ende und Dauer).
//!
//! Eine Komponente beschreibt Rollen (`start`, `end`, `duration`), keine konkreten Felder.
//! Eine Verwendung (`ComponentUse`) ordnet die Rollen realen Feldern einer Entitaet zu.
//! [`expand`] erzeugt daraus beim Laden der DDIC die Atom-Felder, die Nebenwirkungen
//! (`side_effects`) und die ausgeblendeten Felder. Alles Weitere (Fragment, Controller,
//! Skript) leitet die UI-/Script-Schicht aus denselben Angaben ab.
//!
//! Die Expansion ist idempotent: Ein Feld, das bereits mit dem erwarteten Typ existiert
//! (z. B. aus einer zuvor gespeicherten, expandierten DDIC), wird uebernommen.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{EntitySpec, FieldSpec};

/// Art der Komponente. Jede Art hat feste Rollen und eine feste Feldart je Rolle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentKind {
    /// Zeitraum: Rollen `start` und `end` (Edm.DateTimeOffset), `duration` (Edm.Int32).
    TimeRange,
}

/// Einheit der Dauer-Rolle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeUnit {
    /// Ganze Minuten (Standard).
    #[default]
    Minutes,
}

/// Definition einer Komponente (einmal, global).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentSpec {
    pub name: String,
    pub kind: ComponentKind,
    /// Rhai-Modul mit den Regeln, relativ zum Skriptverzeichnis (z. B. `components/TimeRange`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Einheit der Dauer. Standard: Minuten.
    #[serde(default)]
    pub unit: TimeUnit,
    /// Schrittweite der Zeitpunkte in Minuten (1..=60, teilt 60). Standard: 5.
    #[serde(default = "default_step")]
    pub step_minutes: u32,
}

fn default_step() -> u32 {
    5
}

/// Verwendung einer Komponente in einer Entitaet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentUse {
    /// Entitaet, in der die Komponente liegt.
    pub entity: String,
    /// Name der Komponente (`ComponentSpec::name`).
    pub component: String,
    /// Schluessel der Verwendung innerhalb der Entitaet (z. B. `termin`, `ist`).
    pub instance: String,
    /// Rolle -> Feldname. Alle Rollen der Komponente muessen belegt sein.
    pub fields: BTreeMap<String, String>,
    /// Rolle -> Beschriftung. Fehlt eine Rolle, gilt die Standardbeschriftung.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Ueberschreibt die Schrittweite der Komponente fuer diese Verwendung.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_minutes: Option<u32>,
    /// Titel des Abschnitts auf der Object Page. Fehlt er, erscheint kein Abschnitt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_title: Option<String>,
}

/// Rollen der Komponente `kind`, in fester Reihenfolge.
pub fn roles(kind: ComponentKind) -> &'static [&'static str] {
    match kind {
        ComponentKind::TimeRange => &["start", "end", "duration"],
    }
}

/// Feldart je Rolle.
fn edm_type(kind: ComponentKind, role: &str) -> &'static str {
    match (kind, role) {
        (ComponentKind::TimeRange, "duration") => "Edm.Int32",
        (ComponentKind::TimeRange, _) => "Edm.DateTimeOffset",
    }
}

/// Standardbeschriftung je Rolle.
pub fn default_label(kind: ComponentKind, role: &str) -> &'static str {
    match (kind, role) {
        (ComponentKind::TimeRange, "start") => "Beginn",
        (ComponentKind::TimeRange, "end") => "Ende",
        (ComponentKind::TimeRange, _) => "Dauer (Minuten)",
    }
}

/// Effektive Schrittweite einer Verwendung.
pub fn effective_step(component: &ComponentSpec, usage: &ComponentUse) -> u32 {
    usage.step_minutes.unwrap_or(component.step_minutes)
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Prueft Komponenten und Verwendungen gegen die Entitaeten.
pub fn validate(
    entities: &[EntitySpec],
    components: &[ComponentSpec],
    uses: &[ComponentUse],
) -> Result<(), String> {
    let mut names = std::collections::HashSet::new();
    for c in components {
        if !valid_name(&c.name) {
            return Err(format!("komponente '{}': ungueltiger Name", c.name));
        }
        if !names.insert(c.name.as_str()) {
            return Err(format!("komponente '{}' ist doppelt definiert", c.name));
        }
        if c.step_minutes == 0 || c.step_minutes > 60 || 60 % c.step_minutes != 0 {
            return Err(format!(
                "komponente '{}': step_minutes {} muss 1..=60 sein und 60 teilen",
                c.name, c.step_minutes
            ));
        }
    }

    let mut keys = std::collections::HashSet::new();
    for u in uses {
        let ctx = format!("verwendung {}.{}", u.entity, u.instance);
        if !keys.insert((u.entity.as_str(), u.instance.as_str())) {
            return Err(format!("{ctx}: doppelt"));
        }
        if !valid_name(&u.instance) {
            return Err(format!("{ctx}: ungueltiger Schluessel"));
        }
        let Some(component) = components.iter().find(|c| c.name == u.component) else {
            return Err(format!("{ctx}: unbekannte Komponente '{}'", u.component));
        };
        if !entities.iter().any(|e| e.name == u.entity) {
            return Err(format!("{ctx}: unbekannte Entitaet"));
        }
        if let Some(step) = u.step_minutes {
            if step == 0 || step > 60 || 60 % step != 0 {
                return Err(format!(
                    "{ctx}: step_minutes {step} muss 1..=60 sein und 60 teilen"
                ));
            }
        }
        for role in roles(component.kind) {
            let Some(field) = u.fields.get(*role) else {
                return Err(format!("{ctx}: Rolle '{role}' fehlt"));
            };
            if !valid_name(field) {
                return Err(format!("{ctx}: Feldname '{field}' ist ungueltig"));
            }
        }
        for role in u.fields.keys() {
            if !roles(component.kind).contains(&role.as_str()) {
                return Err(format!("{ctx}: unbekannte Rolle '{role}'"));
            }
        }
        let mut targets: Vec<&String> = roles(component.kind)
            .iter()
            .map(|r| &u.fields[*r])
            .collect();
        targets.sort();
        targets.dedup();
        if targets.len() != roles(component.kind).len() {
            return Err(format!("{ctx}: zwei Rollen zeigen auf dasselbe Feld"));
        }
        if component.unit != TimeUnit::Minutes {
            return Err(format!("{ctx}: nur Minuten werden unterstuetzt"));
        }
    }
    Ok(())
}

/// Erzeugt die Felder, Nebenwirkungen und ausgeblendeten Felder aller Verwendungen.
/// Setzt zuvor [`validate`] voraus (wird hier erneut aufgerufen).
pub fn expand(
    entities: &mut [EntitySpec],
    components: &[ComponentSpec],
    uses: &[ComponentUse],
) -> Result<(), String> {
    validate(entities, components, uses)?;
    for usage in uses {
        let component = components
            .iter()
            .find(|c| c.name == usage.component)
            .expect("validiert");
        let entity = entities
            .iter_mut()
            .find(|e| e.name == usage.entity)
            .expect("validiert");
        let kind = component.kind;
        let role_list = roles(kind);

        for role in role_list {
            let field_name = usage.fields[*role].clone();
            let expected = edm_type(kind, role);
            let others: Vec<String> = role_list
                .iter()
                .filter(|r| *r != role)
                .map(|r| usage.fields[*r].clone())
                .collect();

            match entity.fields.iter_mut().find(|f| f.name() == field_name) {
                Some(FieldSpec::Atom {
                    edm_type,
                    side_effects,
                    ..
                }) => {
                    if edm_type != expected {
                        return Err(format!(
                            "{}.{}: Feld existiert mit Typ {} statt {}",
                            usage.entity, field_name, edm_type, expected
                        ));
                    }
                    *side_effects = others;
                }
                Some(_) => {
                    return Err(format!(
                        "{}.{}: Feld existiert, ist aber kein einfaches Atom",
                        usage.entity, field_name
                    ));
                }
                None => {
                    let label = usage
                        .labels
                        .get(*role)
                        .map(String::as_str)
                        .unwrap_or(default_label(kind, role));
                    let mut field = FieldSpec::atom(&field_name, label, expected);
                    if let FieldSpec::Atom { side_effects, .. } = &mut field {
                        *side_effects = others;
                    }
                    entity.fields.push(field);
                }
            }
        }

        let ui = entity.ui.get_or_insert_with(Default::default);
        for role in role_list {
            let field_name = &usage.fields[*role];
            if !ui.hidden_fields.contains(field_name) {
                ui.hidden_fields.push(field_name.clone());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
