//! Festwertlisten (Wertelisten) fuer Statuswerte und Ähnliches.
//!
//! Liegen in `ddic.json` unter `value_lists`. Jede Liste hat eine UUID, auf die
//! ein Atom per `AtomValueList::FieldValueList { list_id }` verweist. Zur
//! Laufzeit werden alle Werte in das generierte Set `FieldValueListItemsSet`
//! geschrieben (siehe `DataDictionary::entities`).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Name der generierten Entitaet (ergibt das Set `FieldValueListItemsSet`).
pub const ITEMS_ENTITY: &str = "FieldValueListItems";

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
