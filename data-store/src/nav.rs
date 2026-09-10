//! Aufloesung der deklarierten Navigationen in eine nach Feldnamen
//! indizierte Form. Beide Richtungen einer Navigation werden beruecksichtigt:
//! die Gegenrichtung dreht die Kardinalitaet um.

use std::collections::HashMap;

use crate::datastore::Navigation;

/// Der Entity-Set, unter dem die Draft-Verwaltungsdaten haengen. Er wird nicht
/// wie ein normaler Kind-Set behandelt, weil die Daten im Record selbst liegen.
pub const DRAFT_ADMIN_SET: &str = "DraftAdministrativeData";

/// Eine aus Sicht eines Entity-Sets aufgeloeste Navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavLink {
    /// `true` = 1:1-Beziehung, `false` = Collection.
    pub single: bool,
    /// `true` = Komposition; das Ziel gehoert zum Lebenszyklus der Quelle.
    pub owning: bool,
    /// Ziel-Entity-Set.
    pub to_set: String,
    /// Feld im Ziel-Set, ueber das die Beziehung aufgeloest wird.
    pub to_field: String,
}

impl NavLink {
    /// Kind-Set einer Komposition: eine gehaltene Collection, die kein
    /// Draft-Admin-Set ist.
    pub fn is_composition_child(&self) -> bool {
        self.owning && !self.single && self.to_set != DRAFT_ADMIN_SET
    }

    /// Parent-Set einer Komposition: die n:1-Gegenrichtung zu
    /// [`NavLink::is_composition_child`]. Das Feld traegt die ID des Parents.
    pub fn is_composition_parent(&self) -> bool {
        self.owning && self.single && self.to_set != DRAFT_ADMIN_SET
    }
}

/// Alle Navigationen, die `entity_set` betreffen, indiziert nach dem Feldnamen
/// im `entity_set`.
pub fn nav_links(navigations: &[Navigation], entity_set: &str) -> HashMap<String, NavLink> {
    let mut links = HashMap::new();
    for nav in navigations {
        let Navigation {
            single,
            owning,
            from_set: Some(from_set),
            from_field: Some(from_field),
            to_set: Some(to_set),
            to_field: Some(to_field),
        } = nav
        else {
            continue;
        };

        if from_set == entity_set {
            links.insert(
                from_field.clone(),
                NavLink {
                    single: *single,
                    owning: *owning,
                    to_set: to_set.clone(),
                    to_field: to_field.clone(),
                },
            );
        } else if to_set == entity_set {
            // Gegenrichtung: aus 1:n wird n:1 und umgekehrt.
            links.insert(
                to_field.clone(),
                NavLink {
                    single: !*single,
                    owning: *owning,
                    to_set: from_set.clone(),
                    to_field: from_field.clone(),
                },
            );
        }
    }
    links
}

/// Der Parent-Link einer Komposition, falls vorhanden.
///
/// Die Auswahl ist ueber den Feldnamen stabil sortiert, damit ein Set mit
/// mehreren gehaltenen n:1-Links nicht je nach HashMap-Reihenfolge einen
/// anderen Parent liefert.
pub fn composition_parent(links: &HashMap<String, NavLink>) -> Option<(&String, &NavLink)> {
    links
        .iter()
        .filter(|(_, link)| link.is_composition_parent())
        .min_by_key(|(field, _)| field.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav(single: bool, owning: bool) -> Navigation {
        Navigation {
            single,
            owning,
            from_set: Some("Orders".into()),
            from_field: Some("_Items".into()),
            to_set: Some("OrderItems".into()),
            to_field: Some("Order_ID".into()),
        }
    }

    fn draft_admin_nav() -> Navigation {
        Navigation {
            single: true,
            owning: true,
            from_set: Some("Orders".into()),
            from_field: Some(DRAFT_ADMIN_SET.into()),
            to_set: Some(DRAFT_ADMIN_SET.into()),
            to_field: Some("ID".into()),
        }
    }

    #[test]
    fn resolves_forward_direction() {
        let links = nav_links(&[nav(false, true)], "Orders");
        let link = links.get("_Items").expect("_Items");
        assert!(!link.single);
        assert_eq!(link.to_set, "OrderItems");
        assert!(link.is_composition_child());
    }

    #[test]
    fn reverse_direction_flips_cardinality() {
        let links = nav_links(&[nav(false, true)], "OrderItems");
        let link = links.get("Order_ID").expect("Order_ID");
        assert!(link.single, "n:1 aus Sicht des Kindes");
        assert_eq!(link.to_set, "Orders");
        assert!(!link.is_composition_child(), "Parent ist kein Kind-Set");
        assert!(link.is_composition_parent());
    }

    #[test]
    fn draft_admin_is_neither_child_nor_parent() {
        let navigations = vec![draft_admin_nav()];
        let links = nav_links(&navigations, "Orders");
        assert!(!links[DRAFT_ADMIN_SET].is_composition_child());
        assert!(!links[DRAFT_ADMIN_SET].is_composition_parent());
    }

    /// Regression: der Draft-Admin-Link ist single+owning und darf beim Suchen
    /// des Parents nicht als solcher durchgehen.
    #[test]
    fn composition_parent_ignores_draft_admin() {
        let links = nav_links(&[nav(false, true), draft_admin_nav()], "Orders");
        assert_eq!(composition_parent(&links), None);

        let child_links = nav_links(&[nav(false, true), draft_admin_nav()], "OrderItems");
        let (field, link) = composition_parent(&child_links).expect("parent link");
        assert_eq!(field, "Order_ID");
        assert_eq!(link.to_set, "Orders");
    }

    #[test]
    fn incomplete_navigations_are_ignored() {
        let incomplete = Navigation {
            single: true,
            owning: false,
            from_set: Some("Orders".into()),
            from_field: None,
            to_set: Some("Customers".into()),
            to_field: Some("ID".into()),
        };
        assert!(nav_links(&[incomplete], "Orders").is_empty());
    }
}
