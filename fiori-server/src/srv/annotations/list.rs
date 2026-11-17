use super::{Annotation, Collection, PropertyValue, Record, target_key_of};
use crate::srv::app_entity_names;
use serde_json::{Value, json};
use spec::data_dictionary::DataDictionary;
use spec::{EntitySpec, FieldSpec, ListColumn};
use tracing::info;

/// Listenannotationen einer Entität.
///
/// * Ohne `list_views`: unqualifiziertes `UI.LineItem` mit `name`, ersatzweise dem ersten
///   darstellbaren Feld.
/// * Mit `list_views`: je Ansicht ein `UI.LineItem` (Qualifier = Ansichtsname, die
///   Ansicht `default` ist unqualifiziert). Zu jeder Ansicht ein
///   `UI.PresentationVariant` mit `Visualizations` auf das LineItem; die Varianten
///   sind die Auswahlpunkte in der ListReport-Ansicht (siehe `flp.rs`).
/// * Eine fehlende `default`-Ansicht wird durch dieselbe Fallback-Spalte ersetzt, weil
///   Fiori Elements ein unqualifiziertes LineItem fuer die Standardtabelle braucht.
pub(crate) fn line_item_annotations(
    ddic: &DataDictionary,
    x: &EntitySpec,
    app_entities: &[String],
) -> Vec<Annotation> {
    let views = x.list_views();
    if views.is_empty() {
        return vec![line_item(
            ddic,
            None,
            &[ListColumn::new(default_line_item_column(x))],
            x,
            app_entities,
        )];
    }

    let mut out = Vec::new();
    let default_cols: Vec<ListColumn> = views
        .iter()
        .find(|v| v.is_default())
        .map(|v| v.columns().to_vec())
        .unwrap_or_else(|| vec![ListColumn::new(default_line_item_column(x))]);
    out.push(line_item(ddic, None, &default_cols, x, app_entities));

    for view in views {
        if !view.is_default() {
            out.push(line_item(
                ddic,
                Some(view.name()),
                view.columns(),
                x,
                app_entities,
            ));
        }
        out.push(presentation_variant(view.name()));
    }
    out
}

fn default_line_item_column(entity: &EntitySpec) -> String {
    let displayable = |field: &&FieldSpec| {
        matches!(
            field,
            FieldSpec::Atom { .. }
                | FieldSpec::Lookup {
                    collection: false,
                    ..
                }
        )
    };
    if entity
        .fields()
        .any(|field| field.name() == "name" && displayable(&field))
    {
        return "name".into();
    }
    entity
        .fields()
        .find(displayable)
        .map(|field| field.name().to_string())
        .unwrap_or_else(|| "name".into())
}

/// Ein `UI.LineItem`. Spalten sind Atome und Measures (`Value = name`) oder
/// einwertige Navigationen (`Value = nav/name`, Anzeige des Zieltextes). Eine Prioritaet
/// wird als `UI.Importance` am Datenfeld vermerkt.
fn line_item(
    ddic: &DataDictionary,
    qualifier: Option<&str>,
    columns: &[ListColumn],
    x: &EntitySpec,
    app_entities: &[String],
) -> Annotation {
    let mut coll = Collection::new();
    for column in columns {
        let column_name = column.field();

        let lookup = x.field_by_name(column_name).and_then(|f| f.lookup_entity());

        let record = match lookup {
            // Lookup field pointing to an entity that is part of the application.
            Some(entity) if app_entities.iter().any(|a| a == entity) => {
                // Der Fremdschluessel muss mitgeladen werden, sonst faellt das Mapping auf die
                // ID der Zeile zurueck. Die Spalte ist ausgeblendet (UI.Hidden).
                coll = coll.add_record(
                    Record::new("UI.DataField")
                        .add(PropertyValue::new("Value").path(ddic.fk_column(entity, column_name)))
                        .annotation(Annotation::new("UI.Hidden")),
                );
                intent_link_record(
                    &format!("{column_name}/name"),
                    &ddic.fk_column(entity, column_name),
                    entity,
                    &target_key_of(ddic, entity).0,
                )
            }
            // Lookup field pointing to an entity that is not part of the application.
            Some(_) => data_field(&format!("{column_name}/name"), false),
            // If it is the name column and this entity is part of the application, create an intent link.
            None if column_name == "name" && app_entities.iter().any(|a| a == x.name()) => {
                intent_link_record("name", x.primary_key_name(), x.name(), x.primary_key_name())
            }
            // Fallback for all other cases: regular data field or eventually a link
            None => data_field(column_name, is_link_atom(x, column_name)),
        };
        let record = if !column_name.contains('/') {
            record.add(PropertyValue::new("Label").string(format!(
                "{{@i18n>{}.{}}}",
                x.name(),
                column_name
            )))
        } else {
            record
        };
        let record = match column.priority() {
            Some(priority) => record.annotation(
                Annotation::new("UI.Importance").enum_member(priority.importance_member()),
            ),
            None => record,
        };
        coll = coll.add_record(record);
    }
    for action in x.actions().iter().filter(|action| action.list) {
        coll = coll.add_record(
            Record::new("UI.DataFieldForAction")
                .add(PropertyValue::new("Action").string(format!("Service.{}", action.name)))
                .add(PropertyValue::new("Label").string(&action.label)),
        );
    }
    let mut ann = Annotation::new("UI.LineItem").collection(coll);
    ann.qualifier = qualifier.map(str::to_string);
    ann
}

/// Spaltenbreiten der Listenansichten fuer die `controlConfiguration` des ListReport.
///
/// Breiten sind keine Annotation. Fiori Elements erwartet sie unter
/// `@UI.LineItem[#<ansicht>]` -> `columns` -> Schluessel der Spalte (`<RecordTyp>::<Pfad>`).
/// Liefert `None`, wenn keine Spalte eine Breite hat.
pub(crate) fn list_column_widths(ddic: &DataDictionary, entity: &str) -> Option<Value> {
    let x = ddic.active_entity(entity)?;
    let app_entities = app_entity_names(ddic);
    let mut out = serde_json::Map::new();
    for view in x.list_views() {
        let mut columns = serde_json::Map::new();
        for column in view.columns() {
            if let Some(width) = column.width() {
                columns.insert(
                    column_record_key(x, column.field(), &app_entities),
                    json!({ "width": width }),
                );
            }
        }
        if columns.is_empty() {
            continue;
        }
        let key = if view.is_default() {
            "@com.sap.vocabularies.UI.v1.LineItem".to_string()
        } else {
            format!("@com.sap.vocabularies.UI.v1.LineItem#{}", view.name())
        };
        out.insert(key, json!({ "columns": columns }));
    }
    (!out.is_empty()).then(|| Value::Object(out))
}

/// Schluessel einer Spalte in der Fiori-Elements-Konfiguration. Entspricht dem Record-Typ,
/// den `line_item` fuer diese Spalte erzeugt.
fn column_record_key(x: &EntitySpec, col: &str, app_entities: &[String]) -> String {
    let lookup = x.fields().find_map(|f| match f {
        spec::FieldSpec::Lookup {
            name,
            entity,
            collection: false,
            ..
        } if name == col => Some(entity.as_str()),
        _ => None,
    });
    match lookup {
        Some(entity) if app_entities.iter().any(|a| a == entity) => stable_column_key(
            "DataFieldWithIntentBasedNavigation",
            &format!("{col}/name"),
            Some((entity, "Detail")),
        ),
        Some(_) => stable_column_key("DataField", &format!("{col}/name"), None),
        None if col == "name" && app_entities.iter().any(|a| a == x.name()) => stable_column_key(
            "DataFieldWithIntentBasedNavigation",
            "name",
            Some((x.name(), "Detail")),
        ),
        None if is_link_atom(x, col) => format!("DataFieldWithUrl::{col}"),
        None => format!("DataField::{col}"),
    }
}

/// Stabiler Spaltenschluessel wie ihn Fiori Elements (UI5 1.153) bildet: Pfad-Separatoren `/`
/// werden zu `::`, da nur `A-Za-z0-9_-:.` als Schluessel zulaessig ist. Mit `intent` wird
/// der Semantic-Object-/Action-Anteil angehaengt.
fn stable_column_key(record: &str, path: &str, intent: Option<(&str, &str)>) -> String {
    let mut key = format!("{record}::{}", path.replace('/', "::"));
    if let Some((entity, action)) = intent {
        key.push_str(&format!("::{entity}::{action}"));
    }
    key
}

/// Semantischer Sprung (`DataFieldWithIntentBasedNavigation`) fuer den Wert `value`,
/// Schluessel `local_key` der Zeile wird auf `ID` der Zielentitaet gemappt.
fn intent_link_record(value: &str, local_key: &str, entity: &str, target_key: &str) -> Record {
    Record::new("UI.DataFieldWithIntentBasedNavigation")
        .add(PropertyValue::new("Value").path(value))
        .add(PropertyValue::new("SemanticObject").string(entity))
        .add(PropertyValue::new("Action").string("Detail"))
        .add(
            PropertyValue::new("Mapping").collection(
                Collection::new().add_record(
                    Record::new("Common.SemanticObjectMappingType")
                        .add(PropertyValue::new("LocalProperty").property_path(local_key))
                        .add(PropertyValue::new("SemanticObjectProperty").string(target_key)),
                ),
            ),
        )
}

/// Ist `name` ein Atom mit `link: true` (Wert ist eine URL)?
pub(crate) fn is_link_atom(x: &EntitySpec, name: &str) -> bool {
    x.fields()
        .any(|f| matches!(f, FieldSpec::Atom { name: n, link: true, .. } if n == name))
}

/// `UI.DataField` bzw. bei `link` ein `UI.DataFieldWithUrl`, dessen Ziel der Wert selbst ist.
pub(crate) fn data_field(path: &str, link: bool) -> Record {
    if link {
        Record::new("UI.DataFieldWithUrl")
            .add(PropertyValue::new("Value").path(path))
            .add(PropertyValue::new("Url").path(path))
    } else {
        Record::new("UI.DataField").add(PropertyValue::new("Value").path(path))
    }
}

/// `UI.PresentationVariant` fuer eine Listenansicht. Die Ansicht `default`
/// ist unqualifiziert, alle anderen tragen `#<name>`.
fn presentation_variant(view_name: &str) -> Annotation {
    info!("presentation_variant: view_name={}", view_name);
    let (qualifier, line_item_path) = if view_name == spec::ListView::DEFAULT {
        (None, "@UI.LineItem".to_string())
    } else {
        (
            Some(view_name.to_string()),
            format!("@UI.LineItem#{view_name}"),
        )
    };
    let visualizations = Collection::new().add_annotation_path(line_item_path);
    let mut ann = Annotation::new("UI.PresentationVariant").record(
        Record::new("UI.PresentationVariantType")
            .add(PropertyValue::new("Text").string(view_name))
            .add(PropertyValue::new("Visualizations").collection(visualizations)),
    );
    ann.qualifier = qualifier;
    ann
}
