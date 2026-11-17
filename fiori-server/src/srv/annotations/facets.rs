use super::{Annotation, Collection, PropertyValue, Record, data_field};
use spec::data_dictionary::DataDictionary;
use spec::{EntitySpec, FieldSpec};
use tracing::info;

/// Identifikator der Allgemein-Facette (Feldabschnitt `Main`).
pub const GENERAL_FACET: &str = "General";
/// Identifikator der Sammel-Facette fuer alle nicht genannten Felder.
pub const DEFAULT_FACET: &str = "Default";

/// Pfad des LineItems fuer eine Navigations-Facette. Ohne Facetten-Einstellung
/// (oder mit Ansicht `default`) das unqualifizierte LineItem, sonst `#<ansicht>`.
pub(crate) fn facet_target_path(x: &EntitySpec, nav: &str) -> String {
    info!("facet_target_path: entity={}, nav={}", x.name(), nav);
    match x.facet(nav).and_then(|f| f.view()) {
        Some(view) if view != spec::ListView::DEFAULT => format!("{nav}/@UI.LineItem#{view}"),
        _ => format!("{nav}/@UI.LineItem"),
    }
}

/// Titel einer Facette: der konfigurierte Titel, sonst der Modelltitel der Navigation,
/// sonst der Schluessel.
pub(crate) fn facet_title(x: &EntitySpec, key: &str) -> String {
    if let Some(title) = x.facet(key).and_then(|f| f.title().map(str::to_string)) {
        return title;
    }
    x.fields()
        .find_map(|f| match f {
            FieldSpec::Lookup { name, label, .. } if name == key => {
                Some(label.clone().unwrap_or_else(|| name.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| key.to_string())
}

/// Object-Page-Abschnitte einer Entitaet in der Reihenfolge von `EntitySpec.facets`.
///
/// * Ohne Konfiguration: `General` (alle Felder), danach je to-many Navigation ein Listenabschnitt.
/// * `General` ist immer vorhanden. Steht es nicht in `facets`, wird es vorangestellt.
/// * Navigationen, die nicht konfiguriert sind, folgen am Ende.
/// * Eigene Schluessel mit `fields` erzeugen eine eigene `UI.FieldGroup#<schluessel>`.
/// * `Default` sammelt alle Felder, die in keiner Facette genannt sind.
///
/// Liefert die Facetten-Records und die zugehoerigen FieldGroups.
pub(crate) fn object_page_sections(
    ddic: &DataDictionary,
    x: &EntitySpec,
    hidden: &[String],
    app_entities: &[String],
) -> (Vec<Record>, Vec<Annotation>) {
    let navs: Vec<String> = x
        .fields()
        .filter_map(|f| match f {
            FieldSpec::Lookup {
                name,
                collection: true,
                ..
            } => Some(name.clone()),
            _ => None,
        })
        .collect();
    let mentioned: Vec<&str> = x
        .facets()
        .iter()
        .flat_map(|f| f.fields().iter().map(String::as_str))
        .collect();
    // Langtexte bekommen je einen eigenen Abschnitt (volle Breite), sofern keine
    // Facette sie ausdruecklich nennt.
    let long_texts: Vec<String> = x
        .fields()
        .filter_map(|f| match f {
            FieldSpec::Atom {
                name,
                multiline: true,
                ..
            } if !mentioned.contains(&name.as_str()) => Some(name.clone()),
            _ => None,
        })
        .collect();
    let remainder: Vec<String> = x
        .fields()
        .filter(|f| {
            !matches!(
                f,
                FieldSpec::Lookup {
                    collection: true,
                    ..
                }
            )
        })
        .filter(|f| !mentioned.contains(&f.name()))
        .filter(|f| !long_texts.iter().any(|l| l == f.name()))
        .map(|f| f.name().to_string())
        .collect();
    let general_fields: Vec<String> = x
        .fields()
        .filter(|f| {
            !matches!(
                f,
                FieldSpec::Lookup {
                    collection: true,
                    ..
                }
            )
        })
        .filter(|f| !long_texts.iter().any(|l| l == f.name()))
        .map(|f| f.name().to_string())
        .collect();

    let configured: Vec<&str> = x.facets().iter().map(|f| f.facet()).collect();
    let mut keys: Vec<String> = Vec::new();
    if !configured.contains(&GENERAL_FACET) {
        keys.push(GENERAL_FACET.to_string());
    }
    keys.extend(configured.iter().map(|k| k.to_string()));
    for nav in &navs {
        if !keys.contains(nav) {
            keys.push(nav.clone());
        }
    }
    for long in &long_texts {
        keys.push(format!("{long}{LONG_TEXT_SUFFIX}"));
    }

    let mut records = Vec::new();
    let mut groups = Vec::new();
    for key in &keys {
        if navs.contains(key) {
            records.push(list_facet_record(x, key));
            continue;
        }
        if let Some(long) = key.strip_suffix(LONG_TEXT_SUFFIX) {
            groups.push(field_group(
                ddic,
                key,
                x,
                Some(&[long.to_string()]),
                hidden,
                app_entities,
            ));
            records.push(
                Record::new("UI.ReferenceFacet")
                    .add(PropertyValue::new("Label").string(facet_title(x, long)))
                    .add(PropertyValue::new("ID").string(format!("{long}Section")))
                    .add(
                        PropertyValue::new("Target")
                            .annotation_path(format!("@UI.FieldGroup#{key}")),
                    ),
            );
            continue;
        }
        let (selected, qualifier): (Option<Vec<String>>, &str) = if key == GENERAL_FACET {
            let fields = x
                .facet(key)
                .map(|f| f.fields().to_vec())
                .filter(|f| !f.is_empty())
                .or_else(|| Some(general_fields.clone()));
            (fields, "Main")
        } else if key == DEFAULT_FACET {
            if remainder.is_empty() {
                continue;
            }
            (Some(remainder.clone()), DEFAULT_FACET)
        } else {
            (x.facet(key).map(|f| f.fields().to_vec()), key.as_str())
        };
        groups.push(field_group(
            ddic,
            qualifier,
            x,
            selected.as_deref(),
            hidden,
            app_entities,
        ));

        if key == GENERAL_FACET {
            records.push(
                Record::new("UI.CollectionFacet")
                    .add(PropertyValue::new("Label").string(facet_title(x, key)))
                    .add(PropertyValue::new("ID").string("GeneralSection"))
                    .add(
                        PropertyValue::new("Facets").collection(
                            Collection::new().add_record(
                                Record::new("UI.ReferenceFacet")
                                    .add(PropertyValue::new("Label").string(facet_title(x, key)))
                                    .add(
                                        PropertyValue::new("Target")
                                            .annotation_path("@UI.FieldGroup#Main"),
                                    ),
                            ),
                        ),
                    ),
            );
        } else {
            records.push(
                Record::new("UI.ReferenceFacet")
                    .add(PropertyValue::new("Label").string(facet_title(x, key)))
                    .add(PropertyValue::new("ID").string(format!("{key}Section")))
                    .add(
                        PropertyValue::new("Target")
                            .annotation_path(format!("@UI.FieldGroup#{qualifier}")),
                    ),
            );
        }
    }
    (records, groups)
}

/// Qualifier-Suffix fuer den eigenen Abschnitt eines Langtextfelds.
const LONG_TEXT_SUFFIX: &str = "_LongText";

/// Listenabschnitt (`UI.ReferenceFacet`) fuer eine to-many Navigation.
fn list_facet_record(x: &EntitySpec, nav: &str) -> Record {
    Record::new("UI.ReferenceFacet")
        .add(PropertyValue::new("Label").string(facet_title(x, nav)))
        .add(PropertyValue::new("ID").string(format!("{nav}Section")))
        .add(PropertyValue::new("Target").annotation_path(facet_target_path(x, nav)))
}

/// Einzel-Verweis auf eine App-Entitaet als semantische Verlinkung:
/// `UI.DataFieldWithIntentBasedNavigation` mit Intent `<Entitaet>-display`,
/// Parameter `ID` aus dem Fremdschluessel.
fn lookup_intent_field(ddic: &DataDictionary, name: &str, entity: &str) -> Record {
    let fk = ddic.fk_column(entity, name);
    let target_key = target_key_of(ddic, entity).0;
    Record::new("UI.DataFieldWithIntentBasedNavigation")
        .add(PropertyValue::new("Value").path(&fk))
        .add(PropertyValue::new("SemanticObject").string(entity))
        .add(PropertyValue::new("Action").string("Detail"))
        .add(
            PropertyValue::new("Mapping").collection(
                Collection::new().add_record(
                    Record::new("Common.SemanticObjectMappingType")
                        .add(PropertyValue::new("LocalProperty").property_path(&fk))
                        .add(PropertyValue::new("SemanticObjectProperty").string(target_key)),
                ),
            ),
        )
}

/// FieldGroup mit dem Qualifier `qualifier`. `selected`: Felder in Anzeigereihenfolge,
/// `None` = alle Felder der Entitaet.
pub(crate) fn field_group(
    ddic: &DataDictionary,
    qualifier: &str,
    x: &EntitySpec,
    selected: Option<&[String]>,
    hidden: &[String],
    app_entities: &[String],
) -> Annotation {
    let fields: Vec<&FieldSpec> = match selected {
        Some(names) => names
            .iter()
            .filter_map(|name| x.fields().find(|f| f.name() == name))
            .collect(),
        None => x.fields().collect(),
    };
    // Begleitfelder (Waehrung/Einheit) einer Measure werden vom Measure-Feld mit angezeigt
    // und daher nicht separat in der FieldGroup gefuehrt. Das Feld bleibt im Modell erhalten.
    let companions: Vec<&str> = x
        .fields()
        .filter_map(|f| match f {
            FieldSpec::Measure { unit_field, .. } => Some(unit_field.as_str()),
            _ => None,
        })
        .collect();
    let mut data = Collection::new();
    for record in fields
        .into_iter()
        .filter(|f| !(matches!(f, FieldSpec::Atom { .. }) && companions.contains(&f.name())))
        .filter_map(|f| field_record(ddic, f, hidden, app_entities))
    {
        data = data.add_record(record);
    }
    let mut ann = Annotation::new("UI.FieldGroup");
    ann.qualifier = Some(qualifier.to_string());
    ann.record(Record::new("UI.FieldGroupType").add(PropertyValue::new("Data").collection(data)))
}

/// Primaerschluessel eines Ziels: Name und EDM-Typ (`ID`/Edm.Guid ohne eigenen Schluessel).
pub(crate) fn target_key_of(ddic: &DataDictionary, entity: &str) -> (String, String) {
    match ddic.active_entity(entity) {
        Some(e) => (
            e.primary_key_name().to_string(),
            e.primary_key_type().to_string(),
        ),
        None => ("ID".to_string(), "Edm.Guid".to_string()),
    }
}

/// Ein `UI.DataField` fuer ein Feld. `None` fuer Felder ohne Zeile in der FieldGroup.
pub(crate) fn field_record(
    ddic: &DataDictionary,
    f: &FieldSpec,
    hidden: &[String],
    app_entities: &[String],
) -> Option<Record> {
    match f {
        FieldSpec::Lookup {
            name,
            entity,
            collection,
            ..
        } => {
            if *collection {
                None
            } else if app_entities.iter().any(|a| a == entity) {
                Some(lookup_intent_field(ddic, name, entity))
            } else {
                Some(
                    Record::new("UI.DataField")
                        .add(PropertyValue::new("Value").path(ddic.fk_column(entity, name))),
                )
            }
        }
        FieldSpec::Atom { name, link, .. } => {
            if hidden.iter().any(|h| h == name) {
                None
            } else {
                Some(data_field(name, *link))
            }
        }
        // Betrag/Menge: FE zeigt Wert und Waehrung/Einheit ueber die Measure-Annotation
        // (Begleitfeld). Das Begleitfeld selbst wird nicht separat angezeigt.
        FieldSpec::Measure { name, .. } => {
            if hidden.iter().any(|h| h == name) {
                None
            } else {
                Some(data_field(name, false))
            }
        }
    }
}
