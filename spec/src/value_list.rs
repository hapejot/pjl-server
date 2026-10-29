//! Festwertlisten (Wertelisten) fuer Statuswerte und Ähnliches.
//!
//! Liegen in `ddic.json` unter `value_lists`. Jede Liste hat eine UUID, auf die
//! ein Atom per `AtomValueList::FieldValueList { list_id }` verweist.
//!
//! Zur Laufzeit bekommt jede Liste eine eigene generierte Entitaet, benannt nach
//! der Liste (z. B. `ProjectStatus`). Ein Atom mit Festwertliste wird im Speicher
//! zu einem Lookup auf diese Entitaet (siehe `DataDictionary::apply_value_list_lookups`).
//! Die ID eines Eintrags ist deterministisch (`entry_id`), damit Verweise einen
//! Neustart ueberleben.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Feld mit der Bezeichnung (Anzeige) eines Festwerts.
pub const LABEL_FIELD: &str = "name";
/// Feld mit dem gespeicherten Code.
pub const CODE_FIELD: &str = "value";
/// Feld fuer die Sortierung.
pub const SORT_FIELD: &str = "sort";

/// Stabile ID eines Festwerts: UUIDv5 aus Listen-ID und Code. Die Listen-ID
/// muss vorher validiert sein (siehe [`validate`]).
pub fn entry_id(list_id: &str, code: &str) -> Uuid {
    let namespace = Uuid::parse_str(list_id).expect("Listen-ID ist validiert");
    Uuid::new_v5(&namespace, code.as_bytes())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueList {
    /// UUID der Liste; wird von `AtomValueList::FieldValueList::list_id` referenziert.
    pub id: String,
    /// Anzeigename im Designer, rein informativ.
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ValueEntry>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueEntry {
    /// Gespeicherter Wert (landet im Atom).
    pub code: String,
    pub label: String,
    #[serde(default)]
    pub sort: u32,
}

/// Prueft IDs (gueltige UUID, eindeutig), Namen und Codes je Liste.
pub fn validate(lists: &[ValueList]) -> Result<(), String> {
    let mut ids = std::collections::HashSet::new();
    let mut names = std::collections::HashSet::new();
    for list in lists {
        Uuid::parse_str(&list.id)
            .map_err(|_| format!("value list '{}': id '{}' ist keine UUID", list.name, list.id))?;
        if !ids.insert(list.id.to_lowercase()) {
            return Err(format!("value list id '{}' doppelt", list.id));
        }
        if !names.insert(list.name.clone()) {
            return Err(format!("value list name '{}' doppelt", list.name));
        }
        let mut codes = std::collections::HashSet::new();
        for v in &list.values {
            if !codes.insert(v.code.clone()) {
                return Err(format!("value list '{}': code '{}' doppelt", list.name, v.code));
            }
        }
    }
    Ok(())
}

/// Ob `name` als Entitaetsname taugt (Buchstabe, dann Buchstaben, Ziffern, `_`).
pub fn is_valid_entity_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

